use super::model::{Action, Rule};
use anyhow::{Context as _, Result, anyhow, bail, ensure};
use serde::{Deserialize, Serialize};
use std::{
    ffi::{CStr, CString, c_char},
    io,
    os::windows::ffi::OsStrExt as _,
    ptr,
    sync::OnceLock,
};
use windows_sys::Win32::{
    Foundation::HMODULE,
    System::LibraryLoader::{
        GetProcAddress, LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR, LOAD_LIBRARY_SEARCH_SYSTEM32, LoadLibraryExW,
    },
};

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct Endpoint {
    pub port: u16,
    pub user: String,
    pub password: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Plan {
    pub endpoint: Option<Endpoint>,
    pub rules: Vec<EngineRule>,
    pub fail_open: bool,
    pub ready: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct EngineRule {
    id: u32,
    pattern: String,
    action: Action,
    enabled: bool,
}

impl From<&Rule> for EngineRule {
    fn from(rule: &Rule) -> Self {
        Self {
            id: rule.id,
            pattern: rule.pattern.clone(),
            action: rule.action,
            enabled: rule.enabled,
        }
    }
}

#[repr(C)]
struct ApplicationRule {
    id: u32,
    action: i32,
    process: *const c_char,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize)]
#[repr(C)]
pub struct Counters {
    pub sent: u64,
    pub received: u64,
    pub established: u64,
    pub failed: u64,
    pub connect_ms: u64,
    pub active: u64,
}

type Lifecycle = unsafe extern "C" fn() -> i32;
type Replace = unsafe extern "C" fn(*const ApplicationRule, u32) -> i32;
type Stats = unsafe extern "C" fn(*const u32, *mut Counters, u32) -> i32;
type Match = unsafe extern "C" fn(*const c_char, *const c_char) -> i32;
type AddProxy = unsafe extern "C" fn(i32, *const c_char, u16, *const c_char, *const c_char, i32) -> u32;
type EditProxy = unsafe extern "C" fn(u32, i32, *const c_char, u16, *const c_char, *const c_char, i32) -> i32;

struct Api {
    start: Lifecycle,
    stop: Lifecycle,
    running: Lifecycle,
    replace: Replace,
    stats: Stats,
    match_process: Match,
    add_proxy: AddProxy,
    edit_proxy: EditProxy,
}

static API: OnceLock<Result<Api, String>> = OnceLock::new();

fn api() -> Result<&'static Api> {
    API.get_or_init(|| Api::load().map_err(|error| format!("{error:#}")))
        .as_ref()
        .map_err(|error| anyhow!("{error}"))
}

impl Api {
    fn load() -> Result<Self> {
        let executable = std::env::current_exe()?;
        let directory = executable.parent().context("Missing application directory")?;
        let path = directory
            .join("resources")
            .join("proxybridge")
            .join("ProxyBridgeCore.dll");
        let name: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
        // The module stays loaded until the process exits. Core workers can outlive a failed stop.
        let module = unsafe {
            LoadLibraryExW(
                name.as_ptr(),
                ptr::null_mut(),
                LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_SYSTEM32,
            )
        };
        if module.is_null() {
            return Err(io::Error::last_os_error()).with_context(|| format!("Loading {}", path.display()));
        }

        fn export(module: HMODULE, name: &CStr) -> Result<unsafe extern "system" fn() -> isize> {
            unsafe { GetProcAddress(module, name.as_ptr().cast()) }
                .with_context(|| format!("Missing Core export {}", name.to_string_lossy()))
        }
        macro_rules! get {
            ($name:expr, $ty:ty) => {{
                let function = export(module, $name)?;
                // The signatures match the Windows C ABI in ProxyBridge.h.
                unsafe { std::mem::transmute::<unsafe extern "system" fn() -> isize, $ty>(function) }
            }};
        }
        Ok(Self {
            start: get!(c"ProxyBridge_Start", Lifecycle),
            stop: get!(c"ProxyBridge_Stop", Lifecycle),
            running: get!(c"ProxyBridge_ApplicationRunning", Lifecycle),
            replace: get!(c"ProxyBridge_ReplaceApplicationRules", Replace),
            stats: get!(c"ProxyBridge_GetApplicationStats", Stats),
            match_process: get!(c"ProxyBridge_MatchesProcess", Match),
            add_proxy: get!(c"ProxyBridge_AddProxyConfig", AddProxy),
            edit_proxy: get!(c"ProxyBridge_EditProxyConfig", EditProxy),
        })
    }
}

pub fn matches(pattern: &CString, image: &CString) -> bool {
    api().is_ok_and(|api| unsafe { (api.match_process)(pattern.as_ptr(), image.as_ptr()) != 0 })
}

#[derive(Default)]
pub struct Engine {
    proxy_id: u32,
    started: bool,
    endpoint: Option<Endpoint>,
    ids: Vec<u32>,
}

impl Engine {
    pub fn apply(&mut self, plan: Plan) -> Result<()> {
        let api = api()?;
        ensure!(plan.rules.len() <= 256, "Too many application rules");
        let endpoint = plan.endpoint.unwrap_or_else(|| Endpoint {
            port: 1,
            ..Endpoint::default()
        });
        ensure!(endpoint.port != 0, "Invalid local SOCKS5 port");
        ensure!(
            endpoint.user.len() <= 255 && endpoint.password.len() <= 255,
            "Invalid SOCKS5 credentials"
        );
        let user = CString::new(endpoint.user.as_bytes())?;
        let password = CString::new(endpoint.password.as_bytes())?;

        let rules: Vec<_> = plan.rules.iter().filter(|rule| rule.enabled).collect();
        let mut strings = Vec::with_capacity(rules.len() + 1);
        strings.push(CString::new(
            "verge-mihomo.exe;verge-mihomo-alpha.exe;clash-verge.exe;clash-verge-service.exe",
        )?);
        for rule in &rules {
            ensure!(
                (1..=65535).contains(&rule.id) && !rule.pattern.is_empty(),
                "Invalid application rule"
            );
            ensure!(rule.pattern.len() < 24576, "Application match is too long");
            strings.push(CString::new(rule.pattern.as_bytes())?);
        }
        let wire: Vec<_> = std::iter::once(ApplicationRule {
            id: 0,
            action: 1,
            process: strings[0].as_ptr(),
        })
        .chain(rules.iter().zip(&strings[1..]).map(|(rule, pattern)| ApplicationRule {
            id: rule.id,
            action: match rule.action {
                Action::Proxy if plan.ready => 0,
                Action::Proxy if plan.fail_open => 1,
                Action::Proxy | Action::Block => 2,
                Action::Direct => 1,
            },
            process: pattern.as_ptr(),
        }))
        .collect();

        if self.endpoint.as_ref() != Some(&endpoint) {
            self.stop()?;
            let host = c"127.0.0.1".as_ptr();
            if self.proxy_id == 0 {
                self.proxy_id = unsafe { (api.add_proxy)(1, host, endpoint.port, user.as_ptr(), password.as_ptr(), 0) };
                ensure!(self.proxy_id != 0, "Core rejected the local SOCKS5 endpoint");
            } else {
                ensure!(
                    unsafe {
                        (api.edit_proxy)(
                            self.proxy_id,
                            1,
                            host,
                            endpoint.port,
                            user.as_ptr(),
                            password.as_ptr(),
                            0,
                        )
                    } != 0,
                    "Core could not update the local SOCKS5 endpoint"
                );
            }
            self.endpoint = Some(endpoint);
        }

        ensure!(
            unsafe { (api.replace)(wire.as_ptr(), wire.len() as u32) } != 0,
            "Core rejected the application rules"
        );
        self.ids = plan.rules.iter().map(|rule| rule.id).collect();
        self.start()
    }

    fn start(&mut self) -> Result<()> {
        if !self.started {
            self.started = unsafe { (api()?.start)() } != 0;
            if !self.started {
                bail!("ProxyBridge failed to start: {}", io::Error::last_os_error());
            }
        }
        Ok(())
    }

    pub fn snapshot(&mut self) -> Result<Vec<(u32, Counters)>> {
        let api = api()?;
        if self.started && unsafe { (api.running)() } == 0 {
            self.stop()?;
            self.start()?;
        }
        ensure!(self.started, "ProxyBridge capture stopped");
        let mut counts = vec![Counters::default(); self.ids.len()];
        ensure!(
            unsafe { (api.stats)(self.ids.as_ptr(), counts.as_mut_ptr(), self.ids.len() as u32) } != 0,
            "Could not read ProxyBridge statistics"
        );
        Ok(self.ids.iter().copied().zip(counts).collect())
    }

    pub fn stop(&mut self) -> Result<()> {
        if self.started {
            self.started = false;
            ensure!(
                unsafe { (api()?.stop)() } != 0,
                "Could not stop all ProxyBridge workers"
            );
        }
        Ok(())
    }
}
