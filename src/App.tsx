import { useEffect, useMemo, useState, type ReactNode } from 'react'
import {
  AlertTriangle,
  ArchiveRestore,
  Box,
  CheckCircle2,
  ChevronRight,
  CircleDot,
  FileArchive,
  FilePlus2,
  FileWarning,
  Folder,
  FolderOpen,
  GitCompare,
  HardDrive,
  MoreHorizontal,
  RefreshCw,
  Rocket,
  RotateCcw,
  ShieldCheck,
  Sparkles,
  Trash2,
} from 'lucide-react'
import { invoke } from '@tauri-apps/api/core'
import { open } from '@tauri-apps/plugin-dialog'
import './App.css'

type FileAction = {
  path: string
  sha256?: string
}

type RenameAction = {
  from: string
  to: string
  sha256: string
}

type Conflict = {
  path: string
  reason: string
}

type UpdatePlan = {
  from: string
  to: string
  added: FileAction[]
  removed: FileAction[]
  updated: FileAction[]
  renamed: RenameAction[]
  preserved: string[]
  conflicts: Conflict[]
  backup_candidates: string[]
}

type BackupSummary = {
  id: string
  from: string
  to: string
  file_count: number
}

type ApplyResult = {
  backup_id: string
  plan: UpdatePlan
  state_path: string
}

type UpdateSourceConfig = {
  index_url: string
}

type PortableAsset = {
  url: string
  sha256: string
  size?: number
}

type AppRelease = {
  version: string
  notes?: string
  published_at?: string
  portable: PortableAsset
}

type AppUpdateCheck = {
  current_version: string
  latest_version: string
  update_available: boolean
  release?: AppRelease
}

type DownloadedUpdate = {
  version: string
  archive_path: string
  sha256: string
  downloaded_at: string
}

const emptyPlan: UpdatePlan = {
  from: '1.0.3',
  to: '1.0.4',
  added: [],
  removed: [],
  updated: [],
  renamed: [],
  preserved: [],
  conflicts: [],
  backup_candidates: [],
}

const demoDiff = {
  added: ['mods/emi-1.18.2-0.6.6.jar', 'kubejs/server_scripts/new_vault.js', 'config/loot_integrations.toml'],
  updated: ['mods/the_vault-3.14.0.jar -> 3.15.1.jar', 'defaultconfigs/the_vault-server.toml', 'config/jei/ingredient-list-mod-sort-order.ini'],
  removed: ['mods/old-optimization-mod.jar', 'config/oldmod-common.toml'],
  protected: ['config/xaerominimap.txt', 'kubejs/client_scripts/custom_hotbar.js'],
}

function App() {
  const [instanceDir, setInstanceDir] = useState('')
  const [oldSource, setOldSource] = useState('')
  const [newSource, setNewSource] = useState('')
  const [plan, setPlan] = useState<UpdatePlan | null>(null)
  const [backups, setBackups] = useState<BackupSummary[]>([])
  const [busy, setBusy] = useState('')
  const [message, setMessage] = useState('Select the instance, baseline pack, and target pack, then compare to generate a protected update plan.')
  const [lastApply, setLastApply] = useState<ApplyResult | null>(null)
  const [updateSource, setUpdateSource] = useState('')
  const [appUpdate, setAppUpdate] = useState<AppUpdateCheck | null>(null)
  const [downloadedUpdate, setDownloadedUpdate] = useState<DownloadedUpdate | null>(null)

  const canPreview = Boolean(instanceDir && oldSource && newSource && !busy)
  const currentPlan = plan ?? emptyPlan

  const totals = useMemo(() => {
    const p = plan ?? emptyPlan
    return {
      changed: p.added.length + p.removed.length + p.updated.length + p.renamed.length,
      added: p.added.length,
      updated: p.updated.length + p.renamed.length,
      removed: p.removed.length,
      protected: p.preserved.length,
      conflicts: p.conflicts.length,
      backups: p.backup_candidates.length,
    }
  }, [plan])

  useEffect(() => {
    invoke<UpdateSourceConfig | null>('load_update_source')
      .then((config) => {
        if (config?.index_url) {
          setUpdateSource(config.index_url)
        }
      })
      .catch(() => {
        // Loading update settings is optional; manual input remains available.
      })
  }, [])

  async function pickDirectory(setter: (path: string) => void) {
    const selected = await open({ directory: true, multiple: false })
    if (typeof selected === 'string') {
      setter(selected)
      setPlan(null)
      setLastApply(null)
    }
  }

  async function runPreview() {
    setBusy('compare')
    setMessage('Scanning official sources and building a safe file-level update plan...')
    try {
      const result = await invoke<UpdatePlan>('preview_update', {
        instanceDir,
        oldSource,
        newSource,
      })
      setPlan(result)
      setMessage(`Plan generated: ${result.added.length} added, ${result.updated.length} updated, ${result.removed.length} removed, ${result.conflicts.length} conflicts.`)
      await refreshBackups()
    } catch (error) {
      setMessage(String(error))
    } finally {
      setBusy('')
    }
  }

  async function runApply() {
    setBusy('apply')
    setMessage('Creating backup and applying the protected update plan...')
    try {
      const result = await invoke<ApplyResult>('apply_update', {
        instanceDir,
        oldSource,
        newSource,
      })
      setLastApply(result)
      setPlan(result.plan)
      setMessage(`Update complete. Backup ID: ${result.backup_id}. Conflicts: ${result.plan.conflicts.length}.`)
      await refreshBackups()
    } catch (error) {
      setMessage(String(error))
    } finally {
      setBusy('')
    }
  }

  async function refreshBackups() {
    if (!instanceDir) return
    setBusy('refresh')
    try {
      const result = await invoke<BackupSummary[]>('list_backups', { instanceDir })
      setBackups(result)
      setMessage(result.length ? `Found ${result.length} backup records for this instance.` : 'No backup records found for this instance.')
    } catch (error) {
      setMessage(String(error))
    } finally {
      setBusy('')
    }
  }

  async function rollback(backupId: string) {
    setBusy(`rollback:${backupId}`)
    setMessage(`Rolling back backup ${backupId}...`)
    try {
      const result = await invoke<{ restored_files: number }>('rollback', {
        instanceDir,
        backupId,
      })
      setMessage(`Rollback complete. Restored ${result.restored_files} files.`)
      setPlan(null)
      setLastApply(null)
      await refreshBackups()
    } catch (error) {
      setMessage(String(error))
    } finally {
      setBusy('')
    }
  }

  async function openPackDelta() {
    if (!instanceDir) return
    setBusy('packdelta')
    try {
      await invoke('open_folder', { path: `${instanceDir}\\.packdelta` })
      setMessage('Opened the .packdelta state directory.')
    } catch (error) {
      setMessage(String(error))
    } finally {
      setBusy('')
    }
  }

  async function saveUpdateSource() {
    setBusy('save-app-source')
    try {
      await invoke<UpdateSourceConfig>('save_update_source', { indexUrl: updateSource })
      setMessage('Application update source saved.')
    } catch (error) {
      setMessage(String(error))
    } finally {
      setBusy('')
    }
  }

  async function checkAppUpdate() {
    setBusy('check-app-update')
    setMessage('Checking GitHub release index for Kairos Patch updates...')
    try {
      const result = await invoke<AppUpdateCheck>('check_app_update', { indexUrl: updateSource })
      setAppUpdate(result)
      setDownloadedUpdate(null)
      setMessage(
        result.update_available
          ? `Kairos Patch ${result.latest_version} is available. Current version: ${result.current_version}.`
          : `Kairos Patch is up to date: ${result.current_version}.`,
      )
    } catch (error) {
      setMessage(String(error))
    } finally {
      setBusy('')
    }
  }

  async function downloadAppUpdate() {
    if (!appUpdate?.release) return
    setBusy('download-app-update')
    setMessage(`Downloading Kairos Patch ${appUpdate.release.version} portable package...`)
    try {
      const result = await invoke<DownloadedUpdate>('download_app_update', {
        release: appUpdate.release,
      })
      setDownloadedUpdate(result)
      setMessage(`Portable update downloaded and verified: ${result.archive_path}`)
    } catch (error) {
      setMessage(String(error))
    } finally {
      setBusy('')
    }
  }

  async function installPortableUpdate() {
    if (!downloadedUpdate) return
    setBusy('install-app-update')
    setMessage('Kairos Patch will close, replace the portable files, and restart.')
    try {
      await invoke('install_portable_update', { downloaded: downloadedUpdate })
    } catch (error) {
      setMessage(String(error))
      setBusy('')
    }
  }

  return (
    <main className="app-shell">
      <section className="titlebar">
        <div className="brand-mark">
          <ShieldCheck size={18} />
        </div>
        <span>Kairos Patch</span>
        <div className="window-controls" aria-hidden="true">
          <span />
          <span />
          <span />
        </div>
      </section>

      <section className="layout">
        <aside className="sidebar">
          <div className="brand-block">
            <div className="logo-cube">
              <Box size={28} />
            </div>
            <div>
              <strong>Kairos Patch</strong>
              <small>v0.1.0</small>
            </div>
          </div>

          <nav className="nav-list" aria-label="Primary">
            <a className="active" href="#dashboard"><CircleDot size={16} /> Dashboard</a>
            <a href="#diff"><GitCompare size={16} /> Diff Compare</a>
            <a href="#plan"><FileArchive size={16} /> Update Plan</a>
            <a href="#protection"><ShieldCheck size={16} /> Conflict Guard</a>
            <a href="#backups"><ArchiveRestore size={16} /> Backups</a>
            <a href="#instance"><HardDrive size={16} /> Instance</a>
            <a href="#app-update"><RefreshCw size={16} /> App Update</a>
          </nav>

          <div className="sidebar-card">
            <div className="pack-icon">
              <Box size={22} />
            </div>
            <div>
              <strong>Worlds Vault</strong>
              <span>{currentPlan.from || '1.0.3'} {'->'} {currentPlan.to || '1.0.4'}</span>
            </div>
            <small className="ready-dot">Ready</small>
          </div>
        </aside>

        <section className="workspace">
          <header className="hero-header" id="dashboard">
            <div>
              <p className="eyebrow">Minecraft Pack State Manager</p>
              <h1>Kairos Patch Dashboard</h1>
              <span>{message}</span>
            </div>
            <div className={`status-pill ${busy ? 'busy' : ''}`}>
              <span />
              {busy ? 'Working' : 'Ready'}
            </div>
          </header>

          <section className="source-grid" aria-label="Pack sources">
            <SourceCard
              icon={<HardDrive size={19} />}
              title="User Instance Directory"
              badge="In use"
              badgeTone="green"
              value={instanceDir}
              fallback="E:\\Games\\.minecraft\\versions\\WorldsVault"
              version="Worlds Vault 1.0.3"
              meta="Last scan: 2026-05-18 20:30"
              onPick={() => pickDirectory(setInstanceDir)}
            />
            <SourceCard
              icon={<FolderOpen size={19} />}
              title="Old Official Pack"
              badge="Baseline"
              badgeTone="amber"
              value={oldSource}
              fallback="D:\\Packs\\WorldsVault\\1.0.3"
              version={`Current baseline ${currentPlan.from || '1.0.3'}`}
              meta="Files: 1,248"
              onPick={() => pickDirectory(setOldSource)}
            />
            <SourceCard
              icon={<Sparkles size={19} />}
              title="New Official Pack"
              badge="Target"
              badgeTone="violet"
              value={newSource}
              fallback="D:\\Packs\\WorldsVault\\1.0.4"
              version={`Target version ${currentPlan.to || '1.0.4'}`}
              meta="Files: 1,296"
              onPick={() => pickDirectory(setNewSource)}
            />
          </section>

          <section className="action-row">
            <button type="button" className="action-button primary" onClick={runPreview} disabled={!canPreview}>
              <GitCompare size={18} />
              <span>Compare</span>
              <small>Generate update plan</small>
            </button>
            <button type="button" className="action-button" onClick={runApply} disabled={!canPreview || !plan}>
              <FileArchive size={18} />
              <span>Create Backup</span>
              <small>Protect modified files</small>
            </button>
            <button type="button" className="action-button" onClick={openPackDelta} disabled={!instanceDir || Boolean(busy)}>
              <Folder size={18} />
              <span>Open .packdelta</span>
              <small>Open state folder</small>
            </button>
            <button type="button" className="action-button compact" onClick={refreshBackups} disabled={!instanceDir || Boolean(busy)}>
              <RefreshCw size={18} />
              <span>Refresh Scan</span>
            </button>
          </section>

          <section className="stats-grid" aria-label="Update stats">
            <Metric icon={<FileWarning size={22} />} label="Pending Changes" value={totals.changed} tone="blue" />
            <Metric icon={<FilePlus2 size={22} />} label="Added Files" value={totals.added} tone="green" />
            <Metric icon={<RotateCcw size={22} />} label="Updated Files" value={totals.updated} tone="amber" />
            <Metric icon={<Trash2 size={22} />} label="Removed Files" value={totals.removed} tone="red" />
            <Metric icon={<AlertTriangle size={22} />} label="Conflicts" value={totals.conflicts} tone="violet" />
          </section>

          <section className="content-grid">
            <section className="panel diff-panel" id="diff">
              <PanelHeader title="Diff Preview" subtitle={plan ? `${currentPlan.from} -> ${currentPlan.to}` : 'Preview data until a scan runs'} />
              <DiffView plan={plan} />
            </section>

            <section className="panel plan-panel" id="plan">
              <PanelHeader title="Update Plan" subtitle="Estimated 2-5 min" />
              <Stepper conflicts={totals.conflicts} hasPlan={Boolean(plan)} />
            </section>

            <aside className="right-stack">
              <section className="panel side-panel" id="protection">
                <div className="side-title">
                  <span className="side-icon danger"><AlertTriangle size={22} /></span>
                  <div>
                    <h2>Conflict Protection</h2>
                    <p>{totals.conflicts || 2} conflicts require review</p>
                  </div>
                </div>
                <button type="button" className="ghost-button">View Conflicts</button>
              </section>

              <section className="panel side-panel" id="backups">
                <div className="side-title">
                  <span className="side-icon success"><ArchiveRestore size={22} /></span>
                  <div>
                    <h2>Latest Backup</h2>
                    {backups.length ? (
                      <p>{backups[0].from} {'->'} {backups[0].to} · {backups[0].file_count} files</p>
                    ) : (
                      <p>No backup created yet</p>
                    )}
                  </div>
                </div>
                {backups[0] ? (
                  <button type="button" className="ghost-button" onClick={() => rollback(backups[0].id)} disabled={Boolean(busy)}>
                    Restore
                  </button>
                ) : (
                  <button type="button" className="ghost-button" onClick={refreshBackups} disabled={!instanceDir || Boolean(busy)}>
                    Check
                  </button>
                )}
              </section>

              <section className="panel instance-panel" id="instance">
                <PanelHeader title="Instance Info" />
                <InfoRow label="Pack" value="Worlds Vault" />
                <InfoRow label="Current" value={currentPlan.from || '1.0.3'} />
                <InfoRow label="Target" value={currentPlan.to || '1.0.4'} />
                <InfoRow label="Minecraft" value="1.18.2" />
                <InfoRow label="Loader" value="Forge 40.2.17" />
                {lastApply && <InfoRow label="Last Backup" value={lastApply.backup_id} />}
              </section>

              <section className="panel app-update-panel" id="app-update">
                <PanelHeader
                  title="App Update"
                  subtitle={appUpdate ? `${appUpdate.current_version} -> ${appUpdate.latest_version}` : 'GitHub source'}
                />
                <input
                  className="update-source-input"
                  value={updateSource}
                  onChange={(event) => setUpdateSource(event.target.value)}
                  placeholder="https://github.com/SevenThRe/karios-patch/releases/latest/download/release-index.json"
                />
                <div className="update-controls">
                  <button type="button" className="ghost-button" onClick={saveUpdateSource} disabled={!updateSource || Boolean(busy)}>
                    Save
                  </button>
                  <button type="button" className="ghost-button" onClick={checkAppUpdate} disabled={!updateSource || Boolean(busy)}>
                    Check
                  </button>
                  <button type="button" className="ghost-button" onClick={downloadAppUpdate} disabled={!appUpdate?.release || Boolean(busy)}>
                    Download
                  </button>
                  <button type="button" className="ghost-button" onClick={installPortableUpdate} disabled={!downloadedUpdate || Boolean(busy)}>
                    Apply
                  </button>
                </div>
                <p className="update-status">
                  {downloadedUpdate
                    ? `Verified ${downloadedUpdate.version}`
                    : appUpdate?.update_available
                      ? `Available: ${appUpdate.latest_version}`
                      : 'Portable updates use GitHub release-index.json.'}
                </p>
              </section>
            </aside>
          </section>
        </section>
      </section>

      <footer className="execute-bar">
        <button type="button" className="execute-button" onClick={runApply} disabled={!canPreview || !plan}>
          <Rocket size={20} />
          <span>Execute Update Plan</span>
          <small>Back up first, then safely update the pack</small>
        </button>
        <div className="safety-note">
          <ShieldCheck size={18} />
          Backup-first protection enabled
        </div>
      </footer>
    </main>
  )
}

function SourceCard({
  icon,
  title,
  badge,
  badgeTone,
  value,
  fallback,
  version,
  meta,
  onPick,
}: {
  icon: ReactNode
  title: string
  badge: string
  badgeTone: 'green' | 'amber' | 'violet'
  value: string
  fallback: string
  version: string
  meta: string
  onPick: () => void
}) {
  return (
    <article className="source-card">
      <div className="source-head">
        <div>
          <span className="source-icon">{icon}</span>
          <h2>{title}</h2>
        </div>
        <span className={`badge ${badgeTone}`}>{badge}</span>
      </div>
      <div className="path-field" title={value || fallback}>
        <span>{value || fallback}</span>
        <button type="button" onClick={onPick} aria-label={`Select ${title}`}>
          <Folder size={17} />
        </button>
        <button type="button" aria-label="More options">
          <MoreHorizontal size={17} />
        </button>
      </div>
      <div className="source-meta">
        <span><CheckCircle2 size={16} /> {version}</span>
        <small>{meta}</small>
      </div>
    </article>
  )
}

function Metric({
  icon,
  label,
  value,
  tone,
}: {
  icon: ReactNode
  label: string
  value: number
  tone: 'blue' | 'green' | 'amber' | 'red' | 'violet'
}) {
  return (
    <article className={`metric ${tone}`}>
      <span className="metric-icon">{icon}</span>
      <div>
        <span>{label}</span>
        <strong>{value}</strong>
      </div>
    </article>
  )
}

function PanelHeader({ title, subtitle }: { title: string; subtitle?: string }) {
  return (
    <div className="panel-header">
      <h2>{title}</h2>
      {subtitle && <span>{subtitle}</span>}
    </div>
  )
}

function DiffView({ plan }: { plan: UpdatePlan | null }) {
  const sections = plan
    ? [
      { tone: 'green', title: 'Added', count: plan.added.length, items: plan.added.map((item) => item.path) },
      { tone: 'amber', title: 'Updated', count: plan.updated.length + plan.renamed.length, items: [...plan.updated.map((item) => item.path), ...plan.renamed.map((item) => `${item.from} -> ${item.to}`)] },
      { tone: 'red', title: 'Removed', count: plan.removed.length, items: plan.removed.map((item) => item.path) },
      { tone: 'violet', title: 'Protected User Files', count: plan.preserved.length, items: plan.preserved },
    ]
    : [
      { tone: 'green', title: 'Added', count: 12, items: demoDiff.added },
      { tone: 'amber', title: 'Updated', count: 8, items: demoDiff.updated },
      { tone: 'red', title: 'Removed', count: 3, items: demoDiff.removed },
      { tone: 'violet', title: 'Protected User Files', count: 2, items: demoDiff.protected },
    ]

  return (
    <div className="diff-list">
      {sections.map((section) => (
        <section className={`diff-section ${section.tone}`} key={section.title}>
          <button type="button" className="diff-section-head">
            <span>{section.title} <strong>({section.count})</strong></span>
            <ChevronRight size={17} />
          </button>
          <ul>
            {section.items.slice(0, 3).map((item) => (
              <li key={item}>{item}</li>
            ))}
            {section.count > 3 && <li>... {section.count - 3} more files</li>}
            {!section.items.length && <li>No files in this group</li>}
          </ul>
        </section>
      ))}
    </div>
  )
}

function Stepper({ conflicts, hasPlan }: { conflicts: number; hasPlan: boolean }) {
  const steps = [
    { title: 'Verify', body: 'Check source maps and file integrity' },
    { title: 'Backup', body: 'Copy files that may be modified' },
    { title: 'Apply Changes', body: 'Write official pack changes' },
    { title: 'Resolve Conflicts', body: conflicts ? 'Manual review required' : 'No blocking conflicts detected' },
    { title: 'Finish', body: 'Write state and clean temporary files' },
  ]

  return (
    <ol className="stepper">
      {steps.map((step, index) => (
        <li className={hasPlan || index === 0 ? 'active' : ''} key={step.title}>
          <span>{index + 1}</span>
          <div>
            <strong>{step.title}</strong>
            <p>{step.body}</p>
          </div>
        </li>
      ))}
    </ol>
  )
}

function InfoRow({ label, value }: { label: string; value: string }) {
  return (
    <div className="info-row">
      <span>{label}</span>
      <strong title={value}>{value}</strong>
    </div>
  )
}

export default App
