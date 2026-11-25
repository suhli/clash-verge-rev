import {
  AddRounded,
  AppsOutlined,
  AppsRounded,
  ArrowDownwardRounded,
  ArrowUpwardRounded,
  DeleteOutlineRounded,
  EditRounded,
  FolderOpenRounded,
  LayersRounded,
  PauseCircleOutlineRounded,
  PictureInPictureAltRounded,
  PlayCircleOutlineRounded,
  RefreshRounded,
  RestartAltRounded,
  TuneRounded,
} from '@mui/icons-material'
import {
  Alert,
  Avatar,
  Box,
  Button,
  ButtonGroup,
  Checkbox,
  Chip,
  Divider,
  Grid,
  IconButton,
  List,
  ListItemButton,
  MenuItem,
  Select,
  Slider,
  Stack,
  TextField,
  Tooltip,
  Typography,
  alpha,
  useTheme,
} from '@mui/material'
import { useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import useSWR from 'swr'

import {
  BaseDialog,
  BaseEmpty,
  BasePage,
  BaseSearchBox,
  Switch,
} from '@/components/base'
import {
  type ApplicationOverlay,
  type ApplicationProcess,
  type ApplicationRule,
  type ApplicationSettings,
  listApplicationProcesses,
  overlayDefaults,
  readApplicationProxy,
  resetApplicationProxyTotals,
  saveApplicationProxy,
  selectApplicationExecutables,
} from '@/services/application-proxy'
import { showNotice } from '@/services/notice-service'

const ACTIONS = ['proxy', 'direct', 'block'] as const
const ANCHORS = ['topLeft', 'topRight', 'bottomLeft', 'bottomRight'] as const
const REFRESH_RATES = [250, 500, 1000, 2000] as const

const BEHAVIOR_SWITCHES = ['clickThrough', 'locked', 'hideIdle'] as const
const CONTENT_SWITCHES = ['showRates', 'showTotals', 'showDelay'] as const

const COLOR_DL = '#38BDF8'
const COLOR_UL = '#A78BFA'
const COLOR_OK = '#10B981'
const COLOR_WARN = '#F59E0B'
const COLOR_ERR = '#EF4444'

function newRule(
  name = '',
  pattern = '',
  icon: string | null = null,
): ApplicationRule {
  return {
    id: 0,
    name,
    pattern,
    icon,
    action: 'proxy',
    enabled: true,
    overlay: true,
  }
}

function formatBytes(value = 0) {
  for (const [size, label] of [
    [1024 ** 3, 'GB'],
    [1024 ** 2, 'MB'],
    [1024, 'KB'],
  ] as const) {
    if (value >= size) {
      const scaled = value / size
      return `${scaled >= 100 ? scaled.toFixed(0) : scaled.toFixed(1)} ${label}`
    }
  }
  return `${value} B`
}

function hex(value: number) {
  return '#' + (value & 0xffffff).toString(16).padStart(6, '0')
}

function delayColor(delay: number | null | undefined, running: boolean) {
  if (!running) return COLOR_ERR
  if (delay == null) return COLOR_OK
  if (delay > 150) return COLOR_ERR
  if (delay >= 60) return COLOR_WARN
  return COLOR_OK
}

function isDefaultForeground(color: number) {
  const rgb = color & 0xffffff
  return rgb === 0xf4f4f5 || rgb === 0xf3f6fa
}

function isLightRgb(color: number) {
  const r = (color >> 16) & 0xff
  const g = (color >> 8) & 0xff
  const b = color & 0xff
  return r * 299 + g * 587 + b * 114 > 150_000
}

export default function ApplicationProxyPage() {
  const { t } = useTranslation()
  const theme = useTheme()
  const isDark = theme.palette.mode === 'dark'
  const cardBg = isDark ? '#282a36' : '#ffffff'

  const {
    data: view,
    error,
    mutate: refresh,
  } = useSWR('application-proxy', readApplicationProxy, {
    refreshInterval: (value) => value?.settings.overlay.refreshMs ?? 1000,
  })
  const [editing, setEditing] = useState<ApplicationRule>()
  const [appearance, setAppearance] = useState<ApplicationOverlay>()
  const [processes, setProcesses] = useState<ApplicationProcess[]>()
  const [processesLoading, setProcessesLoading] = useState(false)
  const [processMatcher, setProcessMatcher] = useState<
    (input: string) => boolean
  >(() => () => true)
  const [selected, setSelected] = useState<string[]>([])
  const [saving, setSaving] = useState(false)

  const settings = view?.settings
  const stats = view?.stats

  const totals = useMemo(() => {
    if (!settings || !stats) {
      return {
        uploadRate: 0,
        downloadRate: 0,
        uploadTotal: 0,
        downloadTotal: 0,
        active: 0,
      }
    }
    let uploadRate = 0
    let downloadRate = 0
    let uploadTotal = 0
    let downloadTotal = 0
    let active = 0
    for (const rule of settings.rules) {
      const live = stats.rules[rule.id]
      if (!live) continue
      uploadRate += live.uploadRate
      downloadRate += live.downloadRate
      uploadTotal += live.uploadTotal
      downloadTotal += live.downloadTotal
      active += live.active
    }
    return { uploadRate, downloadRate, uploadTotal, downloadTotal, active }
  }, [settings, stats])

  const filteredProcesses = useMemo(() => {
    if (!processes) return []
    return processes.filter(
      (entry) => processMatcher(entry.name) || processMatcher(entry.path),
    )
  }, [processes, processMatcher])

  async function save(next: ApplicationSettings) {
    setSaving(true)
    try {
      const result = await saveApplicationProxy(next)
      if (result.ok) await refresh(result.value, false)
      return result.ok
    } catch {
      return false
    } finally {
      setSaving(false)
    }
  }

  async function loadProcesses() {
    setProcessesLoading(true)
    try {
      setProcesses(await listApplicationProcesses())
    } catch (failure) {
      showNotice.error(failure)
    } finally {
      setProcessesLoading(false)
    }
  }

  async function selectFiles() {
    if (!settings) return
    try {
      const files = await selectApplicationExecutables()
      if (files?.length) {
        await save({
          ...settings,
          rules: [
            ...settings.rules,
            ...files.map((path) =>
              newRule(
                path
                  .split(/[\\/]/)
                  .pop()
                  ?.replace(/\.exe$/i, '') ?? path,
                path,
              ),
            ),
          ],
        })
      }
    } catch (failure) {
      showNotice.error(failure)
    }
  }

  function updateRule(id: number, patch: Partial<ApplicationRule>) {
    if (!settings) return
    void save({
      ...settings,
      rules: settings.rules.map((rule) =>
        rule.id === id ? { ...rule, ...patch } : rule,
      ),
    })
  }

  function moveRule(index: number, offset: number) {
    if (!settings) return
    const rules = settings.rules.slice()
    const target = index + offset
    if (target < 0 || target >= rules.length) return
    const rule = rules[index]
    rules[index] = rules[target]
    rules[target] = rule
    void save({ ...settings, rules })
  }

  function resetTotals() {
    setSaving(true)
    void resetApplicationProxyTotals()
      .then((result) => refresh(result.value, false))
      .catch(() => undefined)
      .finally(() => setSaving(false))
  }

  const statusActive = Boolean(stats?.running && stats.endpointReady)
  const statusLabel = stats
    ? stats.running
      ? t(
          stats.endpointReady
            ? 'layout.applicationProxy.running'
            : 'layout.applicationProxy.unavailable',
        )
      : t('layout.applicationProxy.stopped')
    : ''

  const previewBgAlpha = appearance
    ? (appearance.opacity / 255).toFixed(2)
    : '0.88'
  const previewScale = appearance ? appearance.scalePercent / 100 : 1
  const previewFontPx = appearance
    ? Math.max(9, Math.round(appearance.fontSize * previewScale))
    : 12
  const previewUseSemantic = appearance
    ? isDefaultForeground(appearance.foreground)
    : true
  const previewLightBg = appearance ? isLightRgb(appearance.background) : false

  return (
    <BasePage
      title={t('layout.applicationProxy.title')}
      header={
        settings && stats ? (
          <Stack direction="row" spacing={1} sx={{ alignItems: 'center' }}>
            <Chip
              size="small"
              label={statusLabel}
              sx={{
                fontWeight: 600,
                height: 24,
                bgcolor: statusActive
                  ? alpha(theme.palette.success.main, 0.14)
                  : stats.running
                    ? alpha(theme.palette.warning.main, 0.14)
                    : alpha(theme.palette.text.secondary, 0.1),
                color: statusActive
                  ? 'success.main'
                  : stats.running
                    ? 'warning.main'
                    : 'text.secondary',
                '& .MuiChip-label': { px: 1.25 },
              }}
            />
            <Tooltip title={t('layout.applicationProxy.resetTotals')} arrow>
              <span>
                <IconButton
                  size="small"
                  color="inherit"
                  disabled={saving}
                  onClick={resetTotals}
                >
                  <RestartAltRounded fontSize="small" />
                </IconButton>
              </span>
            </Tooltip>
            <Tooltip title={t('layout.applicationProxy.overlay')} arrow>
              <IconButton
                size="small"
                color="inherit"
                onClick={() => setAppearance(settings.overlay)}
              >
                <TuneRounded fontSize="small" />
              </IconButton>
            </Tooltip>
          </Stack>
        ) : null
      }
    >
      {error ? (
        <Alert severity="error" sx={{ mb: 1.5, borderRadius: 2 }}>
          {String(error)}
        </Alert>
      ) : null}

      {settings && stats ? (
        <Stack spacing={1.5}>
          {stats.tunConflict ? (
            <Alert severity="warning" sx={{ borderRadius: 2 }}>
              {t('layout.applicationProxy.tunConflict')}
            </Alert>
          ) : null}
          {stats.systemProxy ? (
            <Alert severity="info" sx={{ borderRadius: 2 }}>
              {t('layout.applicationProxy.systemProxy')}
            </Alert>
          ) : null}
          {stats.error ? (
            <Alert
              severity="error"
              sx={{ borderRadius: 2 }}
              action={
                settings.enabled ? (
                  <Button
                    color="inherit"
                    size="small"
                    disabled={saving}
                    onClick={() => void save(settings)}
                  >
                    {t('layout.applicationProxy.retry')}
                  </Button>
                ) : undefined
              }
            >
              {stats.error}
            </Alert>
          ) : null}

          <Grid container spacing={1.5} columns={{ xs: 6, sm: 6, md: 12 }}>
            <Grid size={6}>
              <Box
                sx={{
                  height: '100%',
                  borderRadius: 2,
                  backgroundColor: cardBg,
                  p: 1.5,
                  display: 'flex',
                  flexDirection: 'column',
                  justifyContent: 'space-between',
                  gap: 1.5,
                }}
              >
                <Box
                  sx={{
                    display: 'flex',
                    alignItems: 'center',
                    justifyContent: 'space-between',
                    px: 1.25,
                    py: 1,
                    borderRadius: 1.5,
                    bgcolor: settings.enabled
                      ? alpha(theme.palette.success.main, 0.08)
                      : 'transparent',
                    opacity: stats.tunConflict && !settings.enabled ? 0.6 : 1,
                    transition: 'background-color 0.25s',
                  }}
                >
                  <Box
                    sx={{
                      display: 'flex',
                      alignItems: 'center',
                      minWidth: 0,
                      gap: 1,
                    }}
                  >
                    {settings.enabled ? (
                      <PlayCircleOutlineRounded
                        sx={{ color: 'success.main' }}
                      />
                    ) : (
                      <PauseCircleOutlineRounded
                        sx={{ color: 'text.disabled' }}
                      />
                    )}
                    <Box sx={{ minWidth: 0 }}>
                      <Typography
                        variant="subtitle1"
                        sx={{
                          fontWeight: 600,
                          fontSize: '15px',
                          lineHeight: 1.25,
                        }}
                      >
                        {t('layout.applicationProxy.title')}
                      </Typography>
                      <Typography
                        variant="caption"
                        color="text.secondary"
                        noWrap
                        sx={{ display: 'block' }}
                      >
                        {stats.node
                          ? `${stats.node}${stats.nodeDelay != null ? ` · ${stats.nodeDelay} ms` : ''}`
                          : statusLabel}
                      </Typography>
                    </Box>
                  </Box>
                  <Switch
                    edge="end"
                    checked={settings.enabled}
                    disabled={
                      saving || (stats.tunConflict && !settings.enabled)
                    }
                    onChange={(_, enabled) =>
                      void save({ ...settings, enabled })
                    }
                  />
                </Box>

                <Box
                  sx={{
                    display: 'grid',
                    gridTemplateColumns: 'repeat(3, minmax(0, 1fr))',
                    gap: 1,
                    px: 0.5,
                  }}
                >
                  <Box
                    sx={{
                      px: 1.25,
                      py: 0.9,
                      borderRadius: 1.5,
                      bgcolor: alpha(COLOR_DL, isDark ? 0.08 : 0.07),
                    }}
                  >
                    <Typography
                      variant="caption"
                      sx={{
                        color: COLOR_DL,
                        fontWeight: 600,
                        fontVariantNumeric: 'tabular-nums',
                        display: 'block',
                      }}
                    >
                      ↓ {formatBytes(totals.downloadRate)}/s
                    </Typography>
                    <Typography
                      variant="caption"
                      color="text.secondary"
                      sx={{ fontVariantNumeric: 'tabular-nums' }}
                    >
                      {formatBytes(totals.downloadTotal)}
                    </Typography>
                  </Box>
                  <Box
                    sx={{
                      px: 1.25,
                      py: 0.9,
                      borderRadius: 1.5,
                      bgcolor: alpha(COLOR_UL, isDark ? 0.08 : 0.07),
                    }}
                  >
                    <Typography
                      variant="caption"
                      sx={{
                        color: COLOR_UL,
                        fontWeight: 600,
                        fontVariantNumeric: 'tabular-nums',
                        display: 'block',
                      }}
                    >
                      ↑ {formatBytes(totals.uploadRate)}/s
                    </Typography>
                    <Typography
                      variant="caption"
                      color="text.secondary"
                      sx={{ fontVariantNumeric: 'tabular-nums' }}
                    >
                      {formatBytes(totals.uploadTotal)}
                    </Typography>
                  </Box>
                  <Box
                    sx={{
                      px: 1.25,
                      py: 0.9,
                      borderRadius: 1.5,
                      bgcolor: alpha(theme.palette.text.primary, 0.04),
                    }}
                  >
                    <Typography
                      variant="caption"
                      sx={{
                        fontWeight: 600,
                        fontVariantNumeric: 'tabular-nums',
                        display: 'block',
                      }}
                    >
                      {t('layout.applicationProxy.connections')} {totals.active}
                    </Typography>
                    <Typography
                      variant="caption"
                      color="text.secondary"
                      sx={{ fontVariantNumeric: 'tabular-nums' }}
                    >
                      {stats.nodeDelay != null
                        ? `${stats.nodeDelay} ms`
                        : 'N/A'}
                    </Typography>
                  </Box>
                </Box>
              </Box>
            </Grid>

            <Grid size={6}>
              <Box
                sx={{
                  height: '100%',
                  borderRadius: 2,
                  backgroundColor: cardBg,
                  p: 1.5,
                  display: 'flex',
                  flexDirection: 'column',
                  justifyContent: 'space-between',
                  gap: 0.75,
                }}
              >
                <Box
                  sx={{
                    display: 'flex',
                    alignItems: 'center',
                    justifyContent: 'space-between',
                    px: 0.75,
                    pb: 0.5,
                  }}
                >
                  <Box sx={{ display: 'flex', alignItems: 'center', gap: 1 }}>
                    <Box
                      sx={{
                        width: 28,
                        height: 28,
                        borderRadius: 1.25,
                        display: 'flex',
                        alignItems: 'center',
                        justifyContent: 'center',
                        bgcolor: alpha(theme.palette.primary.main, 0.12),
                        color: 'primary.main',
                      }}
                    >
                      <LayersRounded fontSize="small" />
                    </Box>
                    <Typography
                      variant="subtitle1"
                      sx={{ fontWeight: 600, fontSize: '15px' }}
                    >
                      {t('layout.applicationProxy.overlay')}
                    </Typography>
                  </Box>
                  <Button
                    size="small"
                    variant="outlined"
                    startIcon={<TuneRounded fontSize="small" />}
                    onClick={() => setAppearance(settings.overlay)}
                    sx={{ borderRadius: 1.5, textTransform: 'none' }}
                  >
                    {t('layout.applicationProxy.edit')}
                  </Button>
                </Box>

                <Box
                  sx={{
                    display: 'flex',
                    alignItems: 'center',
                    justifyContent: 'space-between',
                    px: 1,
                    py: 0.65,
                    borderRadius: 1.5,
                    bgcolor: settings.overlay.global
                      ? alpha(theme.palette.primary.main, 0.06)
                      : 'transparent',
                  }}
                >
                  <Typography variant="body2" sx={{ fontWeight: 500 }}>
                    {t('layout.applicationProxy.globalOverlay')}
                  </Typography>
                  <Switch
                    edge="end"
                    checked={settings.overlay.global}
                    disabled={saving}
                    onChange={(_, global) =>
                      void save({
                        ...settings,
                        overlay: { ...settings.overlay, global },
                      })
                    }
                  />
                </Box>

                <Box
                  sx={{
                    display: 'flex',
                    alignItems: 'center',
                    justifyContent: 'space-between',
                    px: 1,
                    py: 0.65,
                    borderRadius: 1.5,
                    bgcolor: settings.overlay.windows
                      ? alpha(theme.palette.primary.main, 0.06)
                      : 'transparent',
                  }}
                >
                  <Typography variant="body2" sx={{ fontWeight: 500 }}>
                    {t('layout.applicationProxy.windowOverlay')}
                  </Typography>
                  <Switch
                    edge="end"
                    checked={settings.overlay.windows}
                    disabled={saving}
                    onChange={(_, windows) =>
                      void save({
                        ...settings,
                        overlay: { ...settings.overlay, windows },
                      })
                    }
                  />
                </Box>
              </Box>
            </Grid>
          </Grid>

          <Box
            sx={{
              borderRadius: 2,
              backgroundColor: cardBg,
              overflow: 'hidden',
            }}
          >
            <Box
              sx={{
                px: 2,
                py: 1.25,
                display: 'flex',
                flexWrap: 'wrap',
                alignItems: 'center',
                justifyContent: 'space-between',
                gap: 1,
                borderBottom: 1,
                borderColor: 'divider',
              }}
            >
              <Stack direction="row" spacing={1} sx={{ alignItems: 'center' }}>
                <Box
                  sx={{
                    width: 32,
                    height: 32,
                    borderRadius: 1.5,
                    display: 'flex',
                    alignItems: 'center',
                    justifyContent: 'center',
                    bgcolor: alpha(theme.palette.primary.main, 0.12),
                    color: 'primary.main',
                  }}
                >
                  <AppsRounded fontSize="small" />
                </Box>
                <Typography
                  variant="subtitle1"
                  sx={{ fontWeight: 600, fontSize: '15px' }}
                >
                  {t('layout.applicationProxy.title')}
                </Typography>
                <Chip
                  size="small"
                  label={settings.rules.length}
                  sx={{
                    height: 20,
                    fontSize: '12px',
                    fontWeight: 600,
                    fontVariantNumeric: 'tabular-nums',
                  }}
                />
              </Stack>

              <Stack
                direction="row"
                spacing={1}
                sx={{ flexWrap: 'wrap', alignItems: 'center' }}
              >
                <Button
                  variant="contained"
                  size="small"
                  startIcon={<AppsRounded fontSize="small" />}
                  disabled={saving || processesLoading}
                  onClick={() => void loadProcesses()}
                  sx={{ borderRadius: 1.5, textTransform: 'none' }}
                >
                  {t('layout.applicationProxy.processes')}
                </Button>
                <Button
                  variant="outlined"
                  size="small"
                  startIcon={<FolderOpenRounded fontSize="small" />}
                  disabled={saving}
                  onClick={() => void selectFiles()}
                  sx={{ borderRadius: 1.5, textTransform: 'none' }}
                >
                  {t('layout.applicationProxy.executable')}
                </Button>
                <Button
                  variant="outlined"
                  size="small"
                  startIcon={<AddRounded fontSize="small" />}
                  disabled={saving}
                  onClick={() => setEditing(newRule())}
                  sx={{ borderRadius: 1.5, textTransform: 'none' }}
                >
                  {t('layout.applicationProxy.manual')}
                </Button>
              </Stack>
            </Box>

            {settings.rules.length === 0 ? (
              <Box sx={{ py: 5 }}>
                <BaseEmpty extra={t('layout.applicationProxy.empty')} />
              </Box>
            ) : (
              <Box>
                {settings.rules.map((rule, index) => {
                  const live = stats.rules[rule.id]
                  const delay =
                    settings.overlay.delay === 'node'
                      ? stats.nodeDelay
                      : live?.connectionDelay
                  const actionColor =
                    rule.action === 'proxy'
                      ? 'primary'
                      : rule.action === 'direct'
                        ? 'success'
                        : 'error'

                  return (
                    <Box
                      key={rule.id}
                      sx={{
                        px: 2,
                        py: 1.25,
                        display: 'flex',
                        flexWrap: 'wrap',
                        alignItems: 'center',
                        gap: 1.5,
                        borderBottom:
                          index === settings.rules.length - 1 ? 0 : 1,
                        borderColor: 'divider',
                        opacity: rule.enabled ? 1 : 0.58,
                        transition: 'background-color 0.15s',
                        '&:hover': {
                          bgcolor: alpha(theme.palette.text.primary, 0.03),
                        },
                      }}
                    >
                      <Avatar
                        src={rule.icon ?? undefined}
                        variant="rounded"
                        sx={{
                          width: 36,
                          height: 36,
                          borderRadius: 1.5,
                          bgcolor: alpha(theme.palette.primary.main, 0.1),
                          color: 'primary.main',
                        }}
                      >
                        <AppsOutlined fontSize="small" />
                      </Avatar>

                      <Box sx={{ flex: '1 1 200px', minWidth: 0 }}>
                        <Stack
                          direction="row"
                          spacing={1}
                          sx={{ alignItems: 'center' }}
                        >
                          <Typography
                            variant="body2"
                            sx={{ fontWeight: 600 }}
                            noWrap
                          >
                            {rule.name}
                          </Typography>
                          <Chip
                            size="small"
                            label={rule.action.toUpperCase()}
                            sx={{
                              height: 19,
                              fontSize: '11px',
                              fontWeight: 700,
                              bgcolor: alpha(
                                theme.palette[actionColor].main,
                                0.14,
                              ),
                              color: `${actionColor}.main`,
                              '& .MuiChip-label': { px: 0.9 },
                            }}
                          />
                        </Stack>
                        <Typography
                          variant="caption"
                          color="text.secondary"
                          noWrap
                          sx={{ display: 'block', mt: 0.2 }}
                          title={rule.pattern}
                        >
                          {rule.pattern}
                        </Typography>
                      </Box>

                      <Stack
                        direction="row"
                        spacing={2}
                        sx={{
                          alignItems: 'center',
                          flexWrap: 'wrap',
                          fontVariantNumeric: 'tabular-nums',
                        }}
                      >
                        <Box sx={{ minWidth: 88 }}>
                          <Typography
                            variant="caption"
                            sx={{
                              color: COLOR_DL,
                              fontWeight: 600,
                              display: 'block',
                            }}
                          >
                            ↓ {formatBytes(live?.downloadRate)}/s
                          </Typography>
                          <Typography variant="caption" color="text.secondary">
                            {formatBytes(live?.downloadTotal)}
                          </Typography>
                        </Box>
                        <Box sx={{ minWidth: 88 }}>
                          <Typography
                            variant="caption"
                            sx={{
                              color: COLOR_UL,
                              fontWeight: 600,
                              display: 'block',
                            }}
                          >
                            ↑ {formatBytes(live?.uploadRate)}/s
                          </Typography>
                          <Typography variant="caption" color="text.secondary">
                            {formatBytes(live?.uploadTotal)}
                          </Typography>
                        </Box>
                        <Box sx={{ minWidth: 68 }}>
                          <Stack
                            direction="row"
                            spacing={0.5}
                            sx={{ alignItems: 'center' }}
                          >
                            <Box
                              sx={{
                                width: 6,
                                height: 6,
                                borderRadius: '50%',
                                bgcolor: delayColor(delay, statusActive),
                                flexShrink: 0,
                              }}
                            />
                            <Typography
                              variant="caption"
                              sx={{ fontWeight: 600 }}
                            >
                              {delay == null ? 'N/A' : `${delay} ms`}
                            </Typography>
                          </Stack>
                          <Typography variant="caption" color="text.secondary">
                            {t('layout.applicationProxy.connections')}{' '}
                            {live?.active ?? 0}
                          </Typography>
                        </Box>
                      </Stack>

                      <Stack
                        direction="row"
                        spacing={0.25}
                        sx={{ alignItems: 'center', ml: 'auto' }}
                      >
                        {rule.action === 'proxy' ? (
                          <Tooltip
                            title={t('layout.applicationProxy.windowOverlay')}
                            arrow
                          >
                            <span>
                              <IconButton
                                size="small"
                                disabled={saving}
                                color={rule.overlay ? 'primary' : 'default'}
                                sx={{
                                  opacity: rule.overlay ? 1 : 0.45,
                                }}
                                onClick={() =>
                                  updateRule(rule.id, {
                                    overlay: !rule.overlay,
                                  })
                                }
                              >
                                <PictureInPictureAltRounded fontSize="small" />
                              </IconButton>
                            </span>
                          </Tooltip>
                        ) : null}
                        <Tooltip title={t('layout.applicationProxy.up')} arrow>
                          <span>
                            <IconButton
                              size="small"
                              disabled={saving || index === 0}
                              onClick={() => moveRule(index, -1)}
                            >
                              <ArrowUpwardRounded fontSize="small" />
                            </IconButton>
                          </span>
                        </Tooltip>
                        <Tooltip
                          title={t('layout.applicationProxy.down')}
                          arrow
                        >
                          <span>
                            <IconButton
                              size="small"
                              disabled={
                                saving || index === settings.rules.length - 1
                              }
                              onClick={() => moveRule(index, 1)}
                            >
                              <ArrowDownwardRounded fontSize="small" />
                            </IconButton>
                          </span>
                        </Tooltip>
                        <Tooltip
                          title={t('layout.applicationProxy.edit')}
                          arrow
                        >
                          <span>
                            <IconButton
                              size="small"
                              disabled={saving}
                              onClick={() => setEditing(rule)}
                            >
                              <EditRounded fontSize="small" />
                            </IconButton>
                          </span>
                        </Tooltip>
                        <Tooltip
                          title={t('layout.applicationProxy.remove')}
                          arrow
                        >
                          <span>
                            <IconButton
                              size="small"
                              disabled={saving}
                              sx={{
                                '&:hover': { color: 'error.main' },
                              }}
                              onClick={() =>
                                void save({
                                  ...settings,
                                  rules: settings.rules.filter(
                                    (item) => item.id !== rule.id,
                                  ),
                                })
                              }
                            >
                              <DeleteOutlineRounded fontSize="small" />
                            </IconButton>
                          </span>
                        </Tooltip>
                        <Box sx={{ pl: 0.5 }}>
                          <Switch
                            checked={rule.enabled}
                            disabled={saving}
                            onChange={(_, enabled) =>
                              updateRule(rule.id, { enabled })
                            }
                          />
                        </Box>
                      </Stack>
                    </Box>
                  )
                })}
              </Box>
            )}
          </Box>
        </Stack>
      ) : null}

      <BaseDialog
        open={processes !== undefined}
        title={t('layout.applicationProxy.processes')}
        contentSx={{ width: 520, maxWidth: '100%', pt: 1 }}
        okBtn={
          selected.length
            ? `${t('layout.applicationProxy.add')} (${selected.length})`
            : t('layout.applicationProxy.add')
        }
        cancelBtn={t('layout.applicationProxy.cancel')}
        disableOk={!selected.length || saving}
        onCancel={() => {
          setProcesses(undefined)
          setSelected([])
        }}
        onClose={() => {
          setProcesses(undefined)
          setSelected([])
        }}
        onOk={() => {
          if (!settings || !processes) return
          const rules = processes
            .filter((entry) => selected.includes(entry.path))
            .map((entry) => newRule(entry.name, entry.path, entry.icon))
          void save({
            ...settings,
            rules: [...settings.rules, ...rules],
          }).then((ok) => {
            if (ok) {
              setProcesses(undefined)
              setSelected([])
            }
          })
        }}
      >
        <Stack direction="row" spacing={1} sx={{ alignItems: 'center', mb: 1 }}>
          <Box sx={{ flex: 1 }}>
            <BaseSearchBox
              placeholder={t('layout.applicationProxy.search')}
              onSearch={(match) => setProcessMatcher(() => match)}
            />
          </Box>
          <Tooltip title={t('layout.applicationProxy.refresh')} arrow>
            <span>
              <IconButton
                size="small"
                disabled={processesLoading}
                onClick={() => void loadProcesses()}
              >
                <RefreshRounded fontSize="small" />
              </IconButton>
            </span>
          </Tooltip>
        </Stack>
        <List
          dense
          sx={{
            maxHeight: 360,
            overflow: 'auto',
            border: 1,
            borderColor: 'divider',
            borderRadius: 1.5,
          }}
        >
          {filteredProcesses.map((entry) => (
            <ListItemButton
              key={entry.path}
              onClick={() =>
                setSelected((keys) =>
                  keys.includes(entry.path)
                    ? keys.filter((path) => path !== entry.path)
                    : [...keys, entry.path],
                )
              }
            >
              <Checkbox
                size="small"
                checked={selected.includes(entry.path)}
                tabIndex={-1}
                sx={{ mr: 0.5 }}
              />
              <Avatar
                src={entry.icon ?? undefined}
                variant="rounded"
                sx={{ width: 24, height: 24, mr: 1.25, borderRadius: 1 }}
              >
                <AppsOutlined fontSize="small" />
              </Avatar>
              <Box sx={{ minWidth: 0, flex: 1 }} title={entry.path}>
                <Typography variant="body2" sx={{ fontWeight: 500 }} noWrap>
                  {entry.name}
                </Typography>
                <Typography
                  noWrap
                  variant="caption"
                  color="text.secondary"
                  sx={{ display: 'block' }}
                >
                  {entry.path}
                </Typography>
              </Box>
            </ListItemButton>
          ))}
        </List>
      </BaseDialog>

      <BaseDialog
        open={editing !== undefined}
        title={t('layout.applicationProxy.edit')}
        contentSx={{ width: 420, maxWidth: '100%' }}
        okBtn={t('layout.applicationProxy.save')}
        cancelBtn={t('layout.applicationProxy.cancel')}
        disableOk={saving || !editing?.name.trim() || !editing.pattern.trim()}
        onCancel={() => setEditing(undefined)}
        onClose={() => setEditing(undefined)}
        onOk={() => {
          if (!settings || !editing) return
          const rules =
            editing.id === 0
              ? [...settings.rules, editing]
              : settings.rules.map((rule) =>
                  rule.id === editing.id ? editing : rule,
                )
          void save({ ...settings, rules }).then((ok) => {
            if (ok) setEditing(undefined)
          })
        }}
      >
        {editing ? (
          <Stack spacing={2} sx={{ pt: 1 }}>
            <TextField
              size="small"
              fullWidth
              label={t('layout.applicationProxy.name')}
              value={editing.name}
              onChange={(event) =>
                setEditing({ ...editing, name: event.target.value })
              }
            />
            <TextField
              size="small"
              fullWidth
              label={t('layout.applicationProxy.pattern')}
              value={editing.pattern}
              onChange={(event) =>
                setEditing({ ...editing, pattern: event.target.value })
              }
            />
            <Box>
              <Typography
                variant="caption"
                color="text.secondary"
                sx={{ display: 'block', mb: 0.75 }}
              >
                {t('layout.applicationProxy.action')}
              </Typography>
              <ButtonGroup size="small" fullWidth>
                {ACTIONS.map((action) => (
                  <Button
                    key={action}
                    variant={
                      editing.action === action ? 'contained' : 'outlined'
                    }
                    onClick={() => setEditing({ ...editing, action })}
                  >
                    {action.toUpperCase()}
                  </Button>
                ))}
              </ButtonGroup>
            </Box>
            <Stack direction="row" spacing={1.5} sx={{ alignItems: 'center' }}>
              <Avatar
                src={editing.icon ?? undefined}
                variant="rounded"
                sx={{ width: 38, height: 38, borderRadius: 1.5 }}
              >
                <AppsOutlined />
              </Avatar>
              <Button
                component="label"
                variant="outlined"
                size="small"
                sx={{ borderRadius: 1.5, textTransform: 'none' }}
              >
                {t('layout.applicationProxy.icon')}
                <input
                  hidden
                  type="file"
                  accept="image/*"
                  onChange={(event) => {
                    const file = event.target.files?.[0]
                    if (!file || file.size > 32768) return
                    const reader = new FileReader()
                    reader.onload = () =>
                      setEditing(
                        (rule) =>
                          rule && { ...rule, icon: String(reader.result) },
                      )
                    reader.readAsDataURL(file)
                  }}
                />
              </Button>
            </Stack>
            <Box
              sx={{
                display: 'flex',
                alignItems: 'center',
                justifyContent: 'space-between',
                pt: 0.5,
              }}
            >
              <Typography variant="body2">
                {t('layout.applicationProxy.windowOverlay')}
              </Typography>
              <Switch
                edge="end"
                checked={editing.overlay}
                onChange={(_, overlay) => setEditing({ ...editing, overlay })}
              />
            </Box>
          </Stack>
        ) : null}
      </BaseDialog>

      <BaseDialog
        open={appearance !== undefined}
        title={t('layout.applicationProxy.overlay')}
        contentSx={{ width: 540, maxWidth: '100%' }}
        okBtn={t('layout.applicationProxy.save')}
        cancelBtn={t('layout.applicationProxy.cancel')}
        disableOk={saving}
        onCancel={() => setAppearance(undefined)}
        onClose={() => setAppearance(undefined)}
        onOk={() => {
          if (appearance && settings) {
            void save({ ...settings, overlay: appearance }).then((ok) => {
              if (ok) setAppearance(undefined)
            })
          }
        }}
      >
        {appearance && settings ? (
          <Stack spacing={2} sx={{ pt: 0.5 }}>
            <Box
              sx={{
                p: 2,
                borderRadius: 2,
                display: 'flex',
                alignItems: 'center',
                justifyContent: 'center',
                bgcolor: isDark
                  ? alpha('#000000', 0.25)
                  : alpha('#000000', 0.04),
                border: 1,
                borderColor: 'divider',
              }}
            >
              <Box
                sx={{
                  display: 'inline-flex',
                  alignItems: 'center',
                  gap: 1.25,
                  px: 1.5,
                  py: 0.6,
                  borderRadius: '999px',
                  bgcolor: alpha(
                    hex(appearance.background),
                    Number(previewBgAlpha),
                  ),
                  border: `1px solid ${previewLightBg ? 'rgba(0,0,0,0.12)' : 'rgba(255,255,255,0.14)'}`,
                  color: hex(appearance.foreground),
                  fontFamily: '"Cascadia Mono", Consolas, monospace',
                  fontSize: `${previewFontPx}px`,
                  fontVariantNumeric: 'tabular-nums',
                  lineHeight: 1.35,
                  whiteSpace: 'nowrap',
                  boxShadow: '0 4px 14px rgba(0,0,0,0.22)',
                }}
              >
                <Box
                  sx={{
                    width: 8,
                    height: 8,
                    borderRadius: '50%',
                    bgcolor: COLOR_OK,
                    boxShadow: `0 0 0 3px ${alpha(COLOR_OK, 0.24)}`,
                    flexShrink: 0,
                  }}
                />
                {appearance.showDelay ? <span>42ms</span> : null}
                {appearance.showRates ? (
                  <>
                    <Box component="span" sx={{ opacity: 0.35 }}>
                      |
                    </Box>
                    <Box
                      component="span"
                      sx={{
                        color: previewUseSemantic ? COLOR_DL : 'inherit',
                      }}
                    >
                      ↓ 1.2 MB/s
                    </Box>
                    <Box
                      component="span"
                      sx={{
                        color: previewUseSemantic ? COLOR_UL : 'inherit',
                      }}
                    >
                      ↑ 24 KB/s
                    </Box>
                  </>
                ) : null}
                {appearance.showTotals ? (
                  <>
                    <Box component="span" sx={{ opacity: 0.35 }}>
                      |
                    </Box>
                    <span>Σ 342 MB</span>
                  </>
                ) : null}
              </Box>
            </Box>

            <Stack spacing={0.5}>
              <Box
                sx={{
                  display: 'flex',
                  alignItems: 'center',
                  justifyContent: 'space-between',
                  py: 0.5,
                }}
              >
                <Typography variant="body2">
                  {t('layout.applicationProxy.globalOverlay')}
                </Typography>
                <Switch
                  edge="end"
                  checked={appearance.global}
                  onChange={(_, global) =>
                    setAppearance({ ...appearance, global })
                  }
                />
              </Box>
              <Box
                sx={{
                  display: 'flex',
                  alignItems: 'center',
                  justifyContent: 'space-between',
                  py: 0.5,
                }}
              >
                <Typography variant="body2">
                  {t('layout.applicationProxy.windowOverlay')}
                </Typography>
                <Switch
                  edge="end"
                  checked={appearance.windows}
                  onChange={(_, windows) =>
                    setAppearance({ ...appearance, windows })
                  }
                />
              </Box>
              <Box
                sx={{
                  display: 'flex',
                  alignItems: 'center',
                  justifyContent: 'space-between',
                  py: 0.5,
                }}
              >
                <Typography variant="body2">
                  {t('layout.applicationProxy.failOpen')}
                </Typography>
                <Switch
                  edge="end"
                  checked={settings.failOpen}
                  disabled={saving}
                  onChange={(_, failOpen) =>
                    void save({ ...settings, failOpen })
                  }
                />
              </Box>
              {BEHAVIOR_SWITCHES.map((key) => (
                <Box
                  key={key}
                  sx={{
                    display: 'flex',
                    alignItems: 'center',
                    justifyContent: 'space-between',
                    py: 0.5,
                  }}
                >
                  <Typography variant="body2">
                    {t(
                      ('layout.applicationProxy.' +
                        key) as 'layout.applicationProxy.clickThrough',
                    )}
                  </Typography>
                  <Switch
                    edge="end"
                    checked={appearance[key]}
                    onChange={(_, checked) =>
                      setAppearance({ ...appearance, [key]: checked })
                    }
                  />
                </Box>
              ))}
            </Stack>

            <Divider />

            <Stack direction="row" spacing={1} sx={{ flexWrap: 'wrap' }}>
              {CONTENT_SWITCHES.map((key) => (
                <Chip
                  key={key}
                  label={t(
                    ('layout.applicationProxy.' +
                      key) as 'layout.applicationProxy.showRates',
                  )}
                  color={appearance[key] ? 'primary' : 'default'}
                  variant={appearance[key] ? 'filled' : 'outlined'}
                  onClick={() =>
                    setAppearance({ ...appearance, [key]: !appearance[key] })
                  }
                  sx={{ borderRadius: 1.5 }}
                />
              ))}
            </Stack>

            <Grid container spacing={1.5} columns={12}>
              <Grid size={6}>
                <Typography
                  variant="caption"
                  color="text.secondary"
                  sx={{ display: 'block', mb: 0.5 }}
                >
                  {t('layout.applicationProxy.refreshRate')}
                </Typography>
                <Select
                  size="small"
                  fullWidth
                  value={appearance.refreshMs}
                  onChange={(event) =>
                    setAppearance({
                      ...appearance,
                      refreshMs: Number(event.target.value),
                    })
                  }
                >
                  {REFRESH_RATES.map((ms) => (
                    <MenuItem key={ms} value={ms}>
                      {ms} ms
                    </MenuItem>
                  ))}
                </Select>
              </Grid>
              <Grid size={6}>
                <Typography
                  variant="caption"
                  color="text.secondary"
                  sx={{ display: 'block', mb: 0.5 }}
                >
                  {t('layout.applicationProxy.delay')}
                </Typography>
                <Select
                  size="small"
                  fullWidth
                  value={appearance.delay}
                  onChange={(event) =>
                    setAppearance({
                      ...appearance,
                      delay: event.target.value as ApplicationOverlay['delay'],
                    })
                  }
                >
                  <MenuItem value="node">
                    {t('layout.applicationProxy.nodeDelay')}
                  </MenuItem>
                  <MenuItem value="connection">
                    {t('layout.applicationProxy.connectionDelay')}
                  </MenuItem>
                </Select>
              </Grid>
            </Grid>

            <Divider />

            <Stack spacing={1.25}>
              <Box>
                <Box
                  sx={{
                    display: 'flex',
                    justifyContent: 'space-between',
                    mb: 0.25,
                  }}
                >
                  <Typography variant="caption" color="text.secondary">
                    {t('layout.applicationProxy.fontSize')}
                  </Typography>
                  <Typography
                    variant="caption"
                    sx={{ fontWeight: 600, fontVariantNumeric: 'tabular-nums' }}
                  >
                    {appearance.fontSize} px
                  </Typography>
                </Box>
                <Slider
                  size="small"
                  min={8}
                  max={36}
                  value={appearance.fontSize}
                  onChange={(_, value) =>
                    setAppearance({
                      ...appearance,
                      fontSize: Array.isArray(value) ? value[0] : value,
                    })
                  }
                />
              </Box>

              <Box>
                <Box
                  sx={{
                    display: 'flex',
                    justifyContent: 'space-between',
                    mb: 0.25,
                  }}
                >
                  <Typography variant="caption" color="text.secondary">
                    {t('layout.applicationProxy.opacity')}
                  </Typography>
                  <Typography
                    variant="caption"
                    sx={{ fontWeight: 600, fontVariantNumeric: 'tabular-nums' }}
                  >
                    {Math.round((appearance.opacity / 255) * 100)}%
                  </Typography>
                </Box>
                <Slider
                  size="small"
                  min={0}
                  max={255}
                  value={appearance.opacity}
                  onChange={(_, value) =>
                    setAppearance({
                      ...appearance,
                      opacity: Array.isArray(value) ? value[0] : value,
                    })
                  }
                />
              </Box>

              <Box>
                <Box
                  sx={{
                    display: 'flex',
                    justifyContent: 'space-between',
                    mb: 0.25,
                  }}
                >
                  <Typography variant="caption" color="text.secondary">
                    {t('layout.applicationProxy.scalePercent')}
                  </Typography>
                  <Typography
                    variant="caption"
                    sx={{ fontWeight: 600, fontVariantNumeric: 'tabular-nums' }}
                  >
                    {appearance.scalePercent}%
                  </Typography>
                </Box>
                <Slider
                  size="small"
                  min={50}
                  max={200}
                  step={5}
                  value={appearance.scalePercent}
                  onChange={(_, value) =>
                    setAppearance({
                      ...appearance,
                      scalePercent: Array.isArray(value) ? value[0] : value,
                    })
                  }
                />
              </Box>
            </Stack>

            <Grid container spacing={1.5} columns={12}>
              <Grid size={6}>
                <TextField
                  size="small"
                  fullWidth
                  type="color"
                  label={t('layout.applicationProxy.foreground')}
                  value={hex(appearance.foreground)}
                  onChange={(event) =>
                    setAppearance({
                      ...appearance,
                      foreground: parseInt(event.target.value.slice(1), 16),
                    })
                  }
                />
              </Grid>
              <Grid size={6}>
                <TextField
                  size="small"
                  fullWidth
                  type="color"
                  label={t('layout.applicationProxy.background')}
                  value={hex(appearance.background)}
                  onChange={(event) =>
                    setAppearance({
                      ...appearance,
                      background: parseInt(event.target.value.slice(1), 16),
                    })
                  }
                />
              </Grid>
            </Grid>

            <Box>
              <Typography
                variant="caption"
                color="text.secondary"
                sx={{ display: 'block', mb: 0.5 }}
              >
                {t('layout.applicationProxy.anchor')}
              </Typography>
              <Select
                size="small"
                fullWidth
                value={appearance.anchor}
                onChange={(event) =>
                  setAppearance({
                    ...appearance,
                    anchor: event.target.value as ApplicationOverlay['anchor'],
                  })
                }
              >
                {ANCHORS.map((anchor) => (
                  <MenuItem key={anchor} value={anchor}>
                    {t(
                      ('layout.applicationProxy.' +
                        anchor) as 'layout.applicationProxy.topLeft',
                    )}
                  </MenuItem>
                ))}
              </Select>
            </Box>

            <Grid container spacing={1.5} columns={12}>
              {(['offsetX', 'offsetY', 'globalX', 'globalY'] as const).map(
                (key) => (
                  <Grid key={key} size={3}>
                    <TextField
                      size="small"
                      fullWidth
                      type="number"
                      label={t(
                        ('layout.applicationProxy.' +
                          key) as 'layout.applicationProxy.offsetX',
                      )}
                      value={appearance[key]}
                      onChange={(event) =>
                        setAppearance({
                          ...appearance,
                          [key]: Number(event.target.value),
                        })
                      }
                    />
                  </Grid>
                ),
              )}
            </Grid>

            <Box sx={{ display: 'flex', justifyContent: 'flex-start' }}>
              <Button
                size="small"
                color="inherit"
                onClick={() =>
                  setAppearance({
                    ...overlayDefaults,
                    global: appearance.global,
                    windows: appearance.windows,
                  })
                }
              >
                {t('layout.applicationProxy.defaults')}
              </Button>
            </Box>
          </Stack>
        ) : null}
      </BaseDialog>
    </BasePage>
  )
}
