import AppsOutlined from '@mui/icons-material/AppsOutlined'
import {
  Alert,
  Avatar,
  Box,
  Button,
  Checkbox,
  Chip,
  Dialog,
  DialogActions,
  DialogContent,
  DialogTitle,
  Divider,
  FormControlLabel,
  List,
  ListItemButton,
  MenuItem,
  Paper,
  Stack,
  Switch,
  TextField,
  Typography,
} from '@mui/material'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import useSWR from 'swr'

import { BasePage } from '@/components/base'
import {
  type ApplicationAction,
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

const actions = ['proxy', 'direct', 'block'] as const
const switches = [
  'clickThrough',
  'locked',
  'hideIdle',
  'showRates',
  'showTotals',
  'showDelay',
] as const
const numericFields = [
  'fontSize',
  'opacity',
  'scalePercent',
  'offsetX',
  'offsetY',
  'globalX',
  'globalY',
] as const
const colors = ['foreground', 'background'] as const

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
    overlay: false,
  }
}

function bytes(value = 0) {
  for (const [size, label] of [
    [1024 ** 3, 'GiB'],
    [1024 ** 2, 'MiB'],
    [1024, 'KiB'],
  ] as const) {
    if (value >= size) return (value / size).toFixed(1) + ' ' + label
  }
  return value + ' B'
}

function hex(value: number) {
  return '#' + value.toString(16).padStart(6, '0')
}

export default function ApplicationProxyPage() {
  const { t } = useTranslation()
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
  const [filter, setFilter] = useState('')
  const [selected, setSelected] = useState<string[]>([])
  const [saving, setSaving] = useState(false)
  const settings = view?.settings
  const stats = view?.stats

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
              newRule(path.split(/[\\/]/).pop() ?? path, path),
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

  return (
    <BasePage title={t('layout.applicationProxy.title')} full>
      {error ? <Alert severity="error">{String(error)}</Alert> : null}
      {settings && stats ? (
        <Stack
          spacing={2}
          sx={{ px: 2, py: 1, overflow: 'auto', height: '100%' }}
        >
          <Stack
            direction="row"
            sx={{ flexWrap: 'wrap', alignItems: 'center', gap: 1 }}
          >
            <FormControlLabel
              label={t('layout.applicationProxy.title')}
              control={
                <Switch
                  checked={settings.enabled}
                  disabled={saving || (stats.tunConflict && !settings.enabled)}
                  onChange={(_, enabled) => void save({ ...settings, enabled })}
                />
              }
            />
            <Chip
              label={
                stats.running
                  ? t(
                      stats.endpointReady
                        ? 'layout.applicationProxy.running'
                        : 'layout.applicationProxy.unavailable',
                    )
                  : t('layout.applicationProxy.stopped')
              }
              color={
                stats.running && stats.endpointReady ? 'success' : 'default'
              }
              size="small"
            />
            <Button
              size="small"
              onClick={() => setAppearance(settings.overlay)}
            >
              {t('layout.applicationProxy.overlay')}
            </Button>
            {settings.enabled && stats.error ? (
              <Button
                size="small"
                disabled={saving}
                onClick={() => void save(settings)}
              >
                {t('layout.applicationProxy.retry')}
              </Button>
            ) : null}
          </Stack>
          {stats.tunConflict ? (
            <Alert severity="warning">
              {t('layout.applicationProxy.tunConflict')}
            </Alert>
          ) : null}
          {stats.systemProxy ? (
            <Alert severity="warning">
              {t('layout.applicationProxy.systemProxy')}
            </Alert>
          ) : null}
          {stats.error ? <Alert severity="error">{stats.error}</Alert> : null}
          <Stack direction="row" sx={{ flexWrap: 'wrap', gap: 1 }}>
            <Button
              variant="contained"
              size="small"
              disabled={saving || processesLoading}
              onClick={() => void loadProcesses()}
            >
              {t('layout.applicationProxy.processes')}
            </Button>
            <Button
              size="small"
              disabled={saving}
              onClick={() => void selectFiles()}
            >
              {t('layout.applicationProxy.executable')}
            </Button>
            <Button
              size="small"
              disabled={saving}
              onClick={() => setEditing(newRule())}
            >
              {t('layout.applicationProxy.manual')}
            </Button>
            <Button
              size="small"
              disabled={saving}
              onClick={() => {
                setSaving(true)
                void resetApplicationProxyTotals()
                  .then((result) => refresh(result.value, false))
                  .catch(() => undefined)
                  .finally(() => setSaving(false))
              }}
            >
              {t('layout.applicationProxy.resetTotals')}
            </Button>
          </Stack>
          {settings.rules.length === 0 ? (
            <Typography color="text.secondary">
              {t('layout.applicationProxy.empty')}
            </Typography>
          ) : null}
          {settings.rules.map((rule, index) => {
            const live = stats.rules[rule.id]
            const delay =
              settings.overlay.delay === 'node'
                ? stats.nodeDelay
                : live?.connectionDelay
            return (
              <Paper key={rule.id} variant="outlined" sx={{ p: 1.5 }}>
                <Stack direction="row" sx={{ alignItems: 'center', gap: 1 }}>
                  <Avatar src={rule.icon ?? undefined} variant="rounded">
                    <AppsOutlined />
                  </Avatar>
                  <Box sx={{ flex: 1, minWidth: 0 }}>
                    <Typography sx={{ fontWeight: 600 }} noWrap>
                      {rule.name}
                    </Typography>
                    <Typography
                      variant="caption"
                      color="text.secondary"
                      noWrap
                      sx={{ display: 'block' }}
                      title={rule.pattern}
                    >
                      {rule.pattern}
                    </Typography>
                  </Box>
                  <Switch
                    size="small"
                    checked={rule.enabled}
                    disabled={saving}
                    onChange={(_, enabled) => updateRule(rule.id, { enabled })}
                  />
                  <Chip size="small" label={rule.action.toUpperCase()} />
                </Stack>
                <Stack direction="row" sx={{ gap: 2, flexWrap: 'wrap', my: 1 }}>
                  <Typography variant="body2">
                    ↑ {bytes(live?.uploadRate)}/s · {bytes(live?.uploadTotal)}
                  </Typography>
                  <Typography variant="body2">
                    ↓ {bytes(live?.downloadRate)}/s ·{' '}
                    {bytes(live?.downloadTotal)}
                  </Typography>
                  <Typography variant="body2">
                    {delay == null ? 'N/A' : delay + ' ms'}
                  </Typography>
                  <Typography variant="body2">
                    {t('layout.applicationProxy.connections')}{' '}
                    {live?.active ?? 0}
                  </Typography>
                </Stack>
                <Stack direction="row" sx={{ gap: 0.5, flexWrap: 'wrap' }}>
                  <Button
                    size="small"
                    disabled={saving}
                    onClick={() => setEditing(rule)}
                  >
                    {t('layout.applicationProxy.edit')}
                  </Button>
                  <Button
                    size="small"
                    disabled={saving || index === 0}
                    onClick={() => moveRule(index, -1)}
                  >
                    {t('layout.applicationProxy.up')}
                  </Button>
                  <Button
                    size="small"
                    disabled={saving || index === settings.rules.length - 1}
                    onClick={() => moveRule(index, 1)}
                  >
                    {t('layout.applicationProxy.down')}
                  </Button>
                  <Button
                    size="small"
                    color="error"
                    disabled={saving}
                    onClick={() =>
                      void save({
                        ...settings,
                        rules: settings.rules.filter(
                          (item) => item.id !== rule.id,
                        ),
                      })
                    }
                  >
                    {t('layout.applicationProxy.remove')}
                  </Button>
                </Stack>
              </Paper>
            )
          })}
        </Stack>
      ) : null}

      <Dialog
        open={processes !== undefined}
        fullWidth
        maxWidth="sm"
        onClose={() => setProcesses(undefined)}
      >
        <DialogTitle>{t('layout.applicationProxy.processes')}</DialogTitle>
        <DialogContent>
          <Stack direction="row" sx={{ gap: 1, my: 1 }}>
            <TextField
              size="small"
              fullWidth
              label={t('layout.applicationProxy.search')}
              value={filter}
              onChange={(event) => setFilter(event.target.value)}
            />
            <Button
              disabled={processesLoading}
              onClick={() => void loadProcesses()}
            >
              {t('layout.applicationProxy.refresh')}
            </Button>
          </Stack>
          <List dense>
            {processes
              ?.filter((entry) =>
                entry.path.toLowerCase().includes(filter.trim().toLowerCase()),
              )
              .map((entry) => (
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
                    checked={selected.includes(entry.path)}
                    tabIndex={-1}
                  />
                  <Avatar
                    src={entry.icon ?? undefined}
                    variant="rounded"
                    sx={{ width: 24, height: 24, mr: 1 }}
                  >
                    <AppsOutlined fontSize="small" />
                  </Avatar>
                  <Box sx={{ minWidth: 0 }} title={entry.path}>
                    <Typography noWrap>{entry.name}</Typography>
                    <Typography noWrap variant="caption" color="text.secondary">
                      {entry.path}
                    </Typography>
                  </Box>
                </ListItemButton>
              ))}
          </List>
        </DialogContent>
        <DialogActions>
          <Button
            onClick={() => {
              setProcesses(undefined)
              setSelected([])
            }}
          >
            {t('layout.applicationProxy.cancel')}
          </Button>
          <Button
            disabled={!selected.length || saving}
            onClick={() => {
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
            {t('layout.applicationProxy.add')}
          </Button>
        </DialogActions>
      </Dialog>

      <Dialog
        open={editing !== undefined}
        fullWidth
        maxWidth="xs"
        onClose={() => setEditing(undefined)}
      >
        <DialogTitle>{t('layout.applicationProxy.edit')}</DialogTitle>
        {editing ? (
          <DialogContent>
            <Stack spacing={2} sx={{ mt: 1 }}>
              <TextField
                size="small"
                label={t('layout.applicationProxy.name')}
                value={editing.name}
                onChange={(event) =>
                  setEditing({ ...editing, name: event.target.value })
                }
              />
              <TextField
                size="small"
                label={t('layout.applicationProxy.pattern')}
                value={editing.pattern}
                onChange={(event) =>
                  setEditing({ ...editing, pattern: event.target.value })
                }
              />
              <TextField
                select
                size="small"
                label={t('layout.applicationProxy.action')}
                value={editing.action}
                onChange={(event) =>
                  setEditing({
                    ...editing,
                    action: event.target.value as ApplicationAction,
                  })
                }
              >
                {actions.map((action) => (
                  <MenuItem key={action} value={action}>
                    {action.toUpperCase()}
                  </MenuItem>
                ))}
              </TextField>
              <Stack direction="row" spacing={1} sx={{ alignItems: 'center' }}>
                <Avatar src={editing.icon ?? undefined}>
                  <AppsOutlined />
                </Avatar>
                <Button component="label" size="small">
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
              <FormControlLabel
                label={t('layout.applicationProxy.windowOverlay')}
                control={
                  <Switch
                    checked={editing.overlay}
                    onChange={(_, overlay) =>
                      setEditing({ ...editing, overlay })
                    }
                  />
                }
              />
            </Stack>
          </DialogContent>
        ) : null}
        <DialogActions>
          <Button onClick={() => setEditing(undefined)}>
            {t('layout.applicationProxy.cancel')}
          </Button>
          <Button
            disabled={
              saving || !editing?.name.trim() || !editing.pattern.trim()
            }
            onClick={() => {
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
            {t('layout.applicationProxy.save')}
          </Button>
        </DialogActions>
      </Dialog>

      <Dialog
        open={appearance !== undefined}
        fullWidth
        maxWidth="sm"
        onClose={() => setAppearance(undefined)}
      >
        <DialogTitle>{t('layout.applicationProxy.overlay')}</DialogTitle>
        {appearance && settings ? (
          <DialogContent>
            <Stack spacing={1.5} sx={{ mt: 1 }}>
              {(['global', 'windows'] as const).map((key) => (
                <FormControlLabel
                  key={key}
                  label={t(
                    ('layout.applicationProxy.' +
                      (key === 'global'
                        ? 'globalOverlay'
                        : 'windowOverlay')) as 'layout.applicationProxy.globalOverlay',
                  )}
                  control={
                    <Switch
                      checked={appearance[key]}
                      onChange={(_, checked) =>
                        setAppearance({ ...appearance, [key]: checked })
                      }
                    />
                  }
                />
              ))}
              <FormControlLabel
                label={t('layout.applicationProxy.failOpen')}
                control={
                  <Switch
                    checked={settings.failOpen}
                    disabled={saving}
                    onChange={(_, failOpen) =>
                      void save({ ...settings, failOpen })
                    }
                  />
                }
              />
              <Divider />
              <Stack direction="row" sx={{ gap: 1, flexWrap: 'wrap' }}>
                {switches.map((key) => (
                  <FormControlLabel
                    key={key}
                    label={t(
                      ('layout.applicationProxy.' +
                        key) as 'layout.applicationProxy.clickThrough',
                    )}
                    control={
                      <Checkbox
                        checked={appearance[key]}
                        onChange={(_, checked) =>
                          setAppearance({ ...appearance, [key]: checked })
                        }
                      />
                    }
                  />
                ))}
              </Stack>
              <Stack direction="row" sx={{ gap: 1, flexWrap: 'wrap' }}>
                {numericFields.map((key) => (
                  <TextField
                    key={key}
                    size="small"
                    type="number"
                    sx={{ width: 130 }}
                    label={t(
                      ('layout.applicationProxy.' +
                        key) as 'layout.applicationProxy.fontSize',
                    )}
                    value={appearance[key]}
                    onChange={(event) =>
                      setAppearance({
                        ...appearance,
                        [key]: Number(event.target.value),
                      })
                    }
                  />
                ))}
                {colors.map((key) => (
                  <TextField
                    key={key}
                    size="small"
                    type="color"
                    sx={{ width: 130 }}
                    label={t(
                      ('layout.applicationProxy.' +
                        key) as 'layout.applicationProxy.foreground',
                    )}
                    value={hex(appearance[key])}
                    onChange={(event) =>
                      setAppearance({
                        ...appearance,
                        [key]: parseInt(event.target.value.slice(1), 16),
                      })
                    }
                  />
                ))}
              </Stack>
              <Stack direction="row" sx={{ gap: 1 }}>
                <TextField
                  select
                  size="small"
                  fullWidth
                  label={t('layout.applicationProxy.refreshRate')}
                  value={appearance.refreshMs}
                  onChange={(event) =>
                    setAppearance({
                      ...appearance,
                      refreshMs: Number(event.target.value),
                    })
                  }
                >
                  {[250, 500, 1000, 2000].map((ms) => (
                    <MenuItem key={ms} value={ms}>
                      {ms} ms
                    </MenuItem>
                  ))}
                </TextField>
                <TextField
                  select
                  size="small"
                  fullWidth
                  label={t('layout.applicationProxy.delay')}
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
                </TextField>
              </Stack>
              <TextField
                select
                size="small"
                label={t('layout.applicationProxy.anchor')}
                value={appearance.anchor}
                onChange={(event) =>
                  setAppearance({
                    ...appearance,
                    anchor: event.target.value as ApplicationOverlay['anchor'],
                  })
                }
              >
                {(
                  ['topLeft', 'topRight', 'bottomLeft', 'bottomRight'] as const
                ).map((anchor) => (
                  <MenuItem key={anchor} value={anchor}>
                    {t(
                      ('layout.applicationProxy.' +
                        anchor) as 'layout.applicationProxy.topLeft',
                    )}
                  </MenuItem>
                ))}
              </TextField>
              <Box
                sx={{
                  alignSelf: 'start',
                  p: 1.5,
                  borderRadius: 1,
                  color: hex(appearance.foreground),
                  bgcolor:
                    hex(appearance.background) +
                    appearance.opacity.toString(16).padStart(2, '0'),
                  fontSize:
                    (appearance.fontSize * appearance.scalePercent) / 100,
                }}
              >
                {t('layout.applicationProxy.preview')}{' '}
                {appearance.showDelay ? ' · 42 ms' : ''}
                <br />
                {appearance.showRates ? '↑ 24 KiB/s   ↓ 1.2 MiB/s' : ''}
                <br />
                {appearance.showTotals ? '↑ 18 MiB   ↓ 342 MiB' : ''}
              </Box>
            </Stack>
          </DialogContent>
        ) : null}
        <DialogActions>
          <Button onClick={() => setAppearance(overlayDefaults)}>
            {t('layout.applicationProxy.defaults')}
          </Button>
          <Button onClick={() => setAppearance(undefined)}>
            {t('layout.applicationProxy.cancel')}
          </Button>
          <Button
            disabled={saving}
            onClick={() => {
              if (appearance && settings)
                void save({ ...settings, overlay: appearance }).then((ok) => {
                  if (ok) setAppearance(undefined)
                })
            }}
          >
            {t('layout.applicationProxy.save')}
          </Button>
        </DialogActions>
      </Dialog>
    </BasePage>
  )
}
