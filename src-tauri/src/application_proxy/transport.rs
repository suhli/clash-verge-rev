use super::engine::{Counters, Engine, Plan};
use anyhow::{Context as _, Result, bail, ensure};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::{
    ffi::OsStr,
    fmt::Write as _,
    io,
    mem::size_of,
    os::{windows::ffi::OsStrExt as _, windows::io::AsRawHandle as _},
    process::ExitCode,
    ptr,
    time::Duration,
};
use tokio::{
    io::{AsyncRead, AsyncReadExt as _, AsyncWrite, AsyncWriteExt as _},
    net::windows::named_pipe::{ClientOptions, NamedPipeServer, ServerOptions},
    time::timeout,
};
use windows_sys::Win32::{
    Foundation::{CloseHandle, HANDLE, LocalFree},
    Security::{Authorization::ConvertStringSecurityDescriptorToSecurityDescriptorW, SECURITY_ATTRIBUTES},
    System::{
        Pipes::{GetNamedPipeClientProcessId, GetNamedPipeServerProcessId},
        Threading::GetProcessId,
    },
    UI::{
        Shell::{SEE_MASK_NOASYNC, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW, ShellExecuteExW},
        WindowsAndMessaging::SW_HIDE,
    },
};

const HELPER_ARGUMENT: &str = "--application-proxy-helper";
const MAX_FRAME: usize = 8 * 1024 * 1024;

#[derive(Deserialize, Serialize)]
enum Request {
    Apply(Plan),
    Sample,
    Shutdown,
}

#[derive(Deserialize, Serialize)]
enum Reply {
    Ready,
    Applied,
    Stats(Vec<(u32, Counters)>),
    Error(String),
}

async fn read<T: DeserializeOwned>(pipe: &mut (impl AsyncRead + Unpin)) -> Result<T> {
    let length = pipe.read_u32_le().await? as usize;
    ensure!(
        (1..=MAX_FRAME).contains(&length),
        "Invalid application proxy frame length"
    );
    let mut body = vec![0; length];
    pipe.read_exact(&mut body).await?;
    Ok(serde_json::from_slice(&body)?)
}

async fn write(pipe: &mut (impl AsyncWrite + Unpin), frame: &impl Serialize) -> Result<()> {
    let body = serde_json::to_vec(frame)?;
    ensure!(body.len() <= MAX_FRAME, "Application proxy frame too large");
    pipe.write_u32_le(body.len() as u32).await?;
    pipe.write_all(&body).await?;
    Ok(())
}

fn wide(value: impl AsRef<OsStr>) -> Vec<u16> {
    value.as_ref().encode_wide().chain([0]).collect()
}

fn pipe_name(pid: u32, nonce: &str) -> String {
    format!(r"\\.\pipe\ClashVerge-ApplicationProxy-{pid}-{nonce}")
}

fn pipe_server(name: &str) -> Result<NamedPipeServer> {
    let mut descriptor = ptr::null_mut();
    let sddl = wide("D:P(A;;GA;;;OW)(A;;GRGW;;;BA)(A;;GRGW;;;SY)");
    if unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(sddl.as_ptr(), 1, &mut descriptor, ptr::null_mut())
    } == 0
    {
        return Err(io::Error::last_os_error()).context("Creating the application proxy pipe ACL");
    }
    let mut attributes = SECURITY_ATTRIBUTES {
        nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: descriptor,
        bInheritHandle: 0,
    };
    let result = unsafe {
        ServerOptions::new()
            .first_pipe_instance(true)
            .reject_remote_clients(true)
            .max_instances(1)
            .create_with_security_attributes_raw(name, (&mut attributes as *mut SECURITY_ATTRIBUTES).cast())
    };
    unsafe { LocalFree(descriptor) };
    Ok(result?)
}

fn launch(pid: u32, nonce: String) -> Result<u32> {
    let exe = wide(std::env::current_exe()?);
    let verb = wide("runas");
    let args = wide(format!("{HELPER_ARGUMENT} {pid} {nonce}"));
    let mut execution = SHELLEXECUTEINFOW {
        cbSize: size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC,
        lpVerb: verb.as_ptr(),
        lpFile: exe.as_ptr(),
        lpParameters: args.as_ptr(),
        nShow: SW_HIDE,
        ..Default::default()
    };
    if unsafe { ShellExecuteExW(&mut execution) } == 0 {
        return Err(io::Error::last_os_error()).context("Starting the elevated application proxy helper");
    }
    let child = unsafe { GetProcessId(execution.hProcess) };
    unsafe { CloseHandle(execution.hProcess) };
    ensure!(child != 0, "Could not identify the application proxy helper");
    Ok(child)
}

pub struct Helper {
    pipe: NamedPipeServer,
}

impl Helper {
    pub async fn start() -> Result<Self> {
        let mut bytes = [0; 16];
        getrandom::fill(&mut bytes).map_err(|error| anyhow::anyhow!("{error}"))?;
        let mut nonce = String::with_capacity(32);
        for byte in bytes {
            write!(nonce, "{byte:02x}")?;
        }
        let pid = std::process::id();
        let mut pipe = pipe_server(&pipe_name(pid, &nonce))?;
        let child = tokio::task::spawn_blocking(move || launch(pid, nonce)).await??;
        timeout(Duration::from_secs(15), pipe.connect()).await??;
        let mut peer = 0;
        ensure!(
            unsafe { GetNamedPipeClientProcessId(pipe.as_raw_handle() as HANDLE, &mut peer) } != 0 && peer == child,
            "Unexpected application proxy pipe client"
        );
        ensure!(
            matches!(timeout(Duration::from_secs(5), read(&mut pipe)).await??, Reply::Ready),
            "Unexpected application proxy handshake"
        );
        Ok(Self { pipe })
    }

    async fn call(&mut self, request: Request, deadline: Duration) -> Result<Reply> {
        let reply = timeout(deadline, async {
            write(&mut self.pipe, &request).await?;
            read(&mut self.pipe).await
        })
        .await
        .context("Application proxy helper timed out")??;
        match reply {
            Reply::Error(error) => bail!("{error}"),
            reply => Ok(reply),
        }
    }

    pub async fn apply(&mut self, plan: Plan) -> Result<()> {
        ensure!(
            matches!(
                self.call(Request::Apply(plan), Duration::from_secs(65)).await?,
                Reply::Applied
            ),
            "Unexpected application proxy response"
        );
        Ok(())
    }

    pub async fn sample(&mut self) -> Result<Vec<(u32, Counters)>> {
        match self.call(Request::Sample, Duration::from_secs(3)).await? {
            Reply::Stats(stats) => Ok(stats),
            _ => bail!("Unexpected application proxy response"),
        }
    }

    pub async fn stop(mut self) {
        let _ = timeout(Duration::from_secs(2), write(&mut self.pipe, &Request::Shutdown)).await;
    }
}

async fn serve(pid: u32, nonce: &str) -> Result<()> {
    let mut pipe = ClientOptions::new().open(pipe_name(pid, nonce))?;
    let mut peer = 0;
    ensure!(
        unsafe { GetNamedPipeServerProcessId(pipe.as_raw_handle() as HANDLE, &mut peer) } != 0 && peer == pid,
        "Unexpected application proxy pipe server"
    );
    write(&mut pipe, &Reply::Ready).await?;
    let mut engine = Engine::default();
    let result = async {
        loop {
            let reply = match read(&mut pipe).await? {
                Request::Apply(plan) => engine.apply(plan).map(|()| Reply::Applied),
                Request::Sample => engine.snapshot().map(Reply::Stats),
                Request::Shutdown => break Ok(()),
            };
            let reply = reply.unwrap_or_else(|error| Reply::Error(format!("{error:#}")));
            write(&mut pipe, &reply).await?;
        }
    }
    .await;
    let stopped = engine.stop();
    result.and(stopped)
}

pub fn run_helper() -> Option<ExitCode> {
    let mut args = std::env::args_os().skip(1);
    if args.next().as_deref() != Some(OsStr::new(HELPER_ARGUMENT)) {
        return None;
    }
    let mut run = || -> Result<()> {
        let pid = args.next().context("Missing application proxy parent PID")?;
        let pid = pid.to_str().context("Invalid parent PID")?.parse()?;
        let nonce = args.next().context("Missing application proxy nonce")?;
        let nonce = nonce.to_str().context("Invalid nonce")?;
        ensure!(
            nonce.len() == 32 && nonce.bytes().all(|byte| byte.is_ascii_hexdigit()),
            "Invalid nonce"
        );
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?
            .block_on(serve(pid, nonce))
    };
    Some(if run().is_ok() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn local_owner_can_complete_the_control_handshake() -> Result<()> {
        let pid = std::process::id();
        let mut secret = [0u8; 16];
        getrandom::fill(&mut secret).map_err(|error| anyhow::anyhow!("{error}"))?;
        let nonce = secret.iter().map(|byte| format!("{byte:02x}")).collect::<String>();
        let mut pipe = pipe_server(&pipe_name(pid, &nonce))?;
        let peer = tokio::spawn(async move { serve(pid, &nonce).await });
        timeout(Duration::from_secs(5), async {
            pipe.connect().await?;
            ensure!(matches!(read(&mut pipe).await?, Reply::Ready));
            write(&mut pipe, &Request::Shutdown).await?;
            peer.await??;
            Ok::<(), anyhow::Error>(())
        })
        .await??;
        Ok(())
    }
}
