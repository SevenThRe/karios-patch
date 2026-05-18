import { useEffect, useMemo, useState, type ReactNode } from 'react'
import {
  AlertTriangle,
  ArchiveRestore,
  Box,
  CalendarDays,
  CheckCircle2,
  ChevronRight,
  CircleDot,
  ExternalLink,
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
  History,
  Trash2,
} from 'lucide-react'
import { invoke } from '@tauri-apps/api/core'
import { getVersion } from '@tauri-apps/api/app'
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
  merged: FileAction[]
  renamed: RenameAction[]
  preserved: string[]
  conflicts: Conflict[]
  backup_candidates: string[]
}

type ManifestFile = {
  path: string
  sha256: string
  size: number
  owner: string
  strategy: string
  type: string
}

type PackManifest = {
  schema_version: number
  pack_id: string
  pack_name: string
  version: string
  mc_version?: string
  loader?: {
    type: string
    version: string
  }
  created_at: string
  files: ManifestFile[]
}

type ManifestDiff = {
  from: string
  to: string
  added: ManifestFile[]
  removed: ManifestFile[]
  updated: Array<{
    old: ManifestFile
    new: ManifestFile
  }>
  renamed: Array<{
    old: ManifestFile
    new: ManifestFile
  }>
  unchanged: ManifestFile[]
}

type CompareResult = {
  old_manifest: PackManifest
  new_manifest: PackManifest
  diff: ManifestDiff
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

type AppPreferences = {
  instance_dir?: string | null
  old_source?: string | null
  new_source?: string | null
  locale?: Locale | null
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

type ChangelogRelease = {
  version: string
  title: string
  body: string
  published_at?: string
  url: string
}

type ActivePage = 'dashboard' | 'diff' | 'plan' | 'backups' | 'instance' | 'updates' | 'changelog'
type Locale = 'zh' | 'en'
type PreferencePathKey = 'instance_dir' | 'old_source' | 'new_source'

const PREFERENCES_STORAGE_KEY = 'kairos-patch:preferences'

const i18n = {
  zh: {
    dashboard: '仪表盘',
    diff: '差异比较',
    plan: '更新计划',
    backups: '备份',
    instance: '实例',
    appUpdate: '应用更新',
    changelog: '更新日志',
    packState: 'Minecraft 整合包状态管理',
    dashboardTitle: 'Kairos Patch 仪表盘',
    updateTitle: '应用更新',
    changelogTitle: '更新日志',
    diffTitle: '差异比较',
    planTitle: '更新计划',
    backupsTitle: '备份',
    instanceTitle: '实例',
    ready: '就绪',
    working: '处理中',
    selectedPack: '当前整合包',
    noPlan: '尚未生成计划',
    planned: '已生成',
    idle: '空闲',
    instanceDir: '用户实例目录',
    oldPack: '旧官方包',
    newPack: '新官方包',
    inUse: '使用中',
    baseline: '基准',
    target: '目标',
    selectInstance: '选择 Minecraft 实例目录',
    selectOld: '选择旧官方包目录或 ZIP',
    selectNew: '选择新官方包目录或 ZIP',
    waitingScan: '等待扫描',
    notScanned: '未扫描',
    required: '必需',
    selected: '目录已选择',
    compare: '比较',
    compareSources: '比较源包',
    createBackup: '创建备份',
    openPackdelta: '打开 .packdelta',
    refreshScan: '刷新扫描',
    generatePlan: '生成更新计划',
    executePlan: '执行更新计划',
    backupFirst: '先备份，再安全更新整合包',
    pendingChanges: '待处理变更',
    addedFiles: '新增文件',
    updatedFiles: '更新文件',
    mergedConfigs: '合并配置',
    removedFiles: '删除文件',
    conflicts: '冲突',
    diffPreview: '差异预览',
    updatePlan: '更新计划',
    conflictProtection: '冲突保护',
    latestBackup: '最近备份',
    instanceInfo: '实例信息',
    noComparison: '尚未加载比较结果',
    noComparisonHint: '选择实例、旧官方包和新官方包后运行比较。',
    runCompareHint: '运行比较以加载真实文件变更',
    estimated: '预计 2-5 分钟',
    checkConflicts: '运行比较以检查冲突',
    noBackup: '尚未创建备份',
    check: '检查',
    restore: '还原',
    save: '保存',
    notes: '日志',
    download: '下载',
    apply: '应用',
    appVersion: '应用版本',
    latestChecked: '最近检查',
    downloaded: '已下载',
    updateSource: '更新源',
    notChecked: '未检查',
    none: '无',
    notConfigured: '未配置',
    loadNotes: '加载日志',
    noChangelog: '尚未加载更新日志',
    noChangelogHint: '从更新源配置的 GitHub 仓库加载发布说明。',
    sourceSelection: '源包选择',
    oldVsNew: '旧包 / 新包',
    diffResult: '差异结果',
    waitingCompare: '等待比较',
    protectedPlan: '受保护更新计划',
    notGenerated: '未生成',
    planDetails: '计划明细',
    backupRecords: '备份记录',
    instanceRequired: '需要实例目录',
    availableBackups: '可用备份',
    rollbackHint: '还原已备份的文件',
    instanceScan: '实例扫描',
    instanceManifest: '实例清单',
    waiting: '等待',
    waitingInstance: '等待实例',
    readyBackupList: '可读取备份',
    scanInstance: '扫描实例',
    buildManifest: '生成本地清单',
    readBackups: '读取 .packdelta 备份清单',
    scanBothPacks: '扫描两个官方包并计算文件差异',
    buildPlanActions: '生成受保护的更新动作',
    backupThenWrite: '先备份，再写入变更',
    portableUpdate: '便携版应用更新',
    currentVersion: '当前版本',
    releaseSource: '发布源',
    githubReleases: 'GitHub 发布',
    updateSourceHint: '便携更新使用已配置的 GitHub release-index.json。',
    verified: '已校验',
    available: '可用',
    sourceDiffEmpty: '尚未加载源包差异',
    sourceDiffHint: '选择旧官方包和新官方包目录后比较源包。',
    noFilesGroup: '此分组没有文件',
    moreFiles: '更多文件',
    protectedUserFiles: '保留的用户文件',
    renamedFiles: '重命名文件',
    lastBackup: '最近备份',
    packName: '整合包名称',
    packId: '整合包 ID',
    version: '版本',
    files: '文件数',
    fileSummaryHint: '扫描实例后查看文件归属和处理策略。',
  },
  en: {
    dashboard: 'Dashboard',
    diff: 'Diff Compare',
    plan: 'Update Plan',
    backups: 'Backups',
    instance: 'Instance',
    appUpdate: 'App Update',
    changelog: 'Changelog',
    packState: 'Minecraft Pack State Manager',
    dashboardTitle: 'Kairos Patch Dashboard',
    updateTitle: 'App Update',
    changelogTitle: 'Changelog',
    diffTitle: 'Diff Compare',
    planTitle: 'Update Plan',
    backupsTitle: 'Backups',
    instanceTitle: 'Instance',
    ready: 'Ready',
    working: 'Working',
    selectedPack: 'Selected Pack',
    noPlan: 'No plan generated',
    planned: 'Planned',
    idle: 'Idle',
    instanceDir: 'User Instance Directory',
    oldPack: 'Old Official Pack',
    newPack: 'New Official Pack',
    inUse: 'In use',
    baseline: 'Baseline',
    target: 'Target',
    selectInstance: 'Select a Minecraft instance directory',
    selectOld: 'Select the previous official pack folder or ZIP',
    selectNew: 'Select the target official pack folder or ZIP',
    waitingScan: 'Waiting for scan',
    notScanned: 'Not scanned',
    required: 'Required',
    selected: 'Directory selected',
    compare: 'Compare',
    compareSources: 'Compare Sources',
    createBackup: 'Create Backup',
    openPackdelta: 'Open .packdelta',
    refreshScan: 'Refresh Scan',
    generatePlan: 'Generate update plan',
    executePlan: 'Execute Update Plan',
    backupFirst: 'Back up first, then safely update the pack',
    pendingChanges: 'Pending Changes',
    addedFiles: 'Added Files',
    updatedFiles: 'Updated Files',
    mergedConfigs: 'Merged Configs',
    removedFiles: 'Removed Files',
    conflicts: 'Conflicts',
    diffPreview: 'Diff Preview',
    updatePlan: 'Update Plan',
    conflictProtection: 'Conflict Protection',
    latestBackup: 'Latest Backup',
    instanceInfo: 'Instance Info',
    noComparison: 'No comparison loaded',
    noComparisonHint: 'Select the instance, old official pack, and new official pack, then run Compare.',
    runCompareHint: 'Run Compare to load real file changes',
    estimated: 'Estimated 2-5 min',
    checkConflicts: 'Run Compare to check conflicts',
    noBackup: 'No backup created yet',
    check: 'Check',
    restore: 'Restore',
    save: 'Save',
    notes: 'Notes',
    download: 'Download',
    apply: 'Apply',
    appVersion: 'App Version',
    latestChecked: 'Latest Checked',
    downloaded: 'Downloaded',
    updateSource: 'Update Source',
    notChecked: 'Not checked',
    none: 'None',
    notConfigured: 'Not configured',
    loadNotes: 'Load Notes',
    noChangelog: 'No changelog loaded',
    noChangelogHint: 'Load release notes from the GitHub repository configured in the update source.',
    sourceSelection: 'Source Selection',
    oldVsNew: 'Old vs New',
    diffResult: 'Diff Result',
    waitingCompare: 'Waiting for compare',
    protectedPlan: 'Protected Update Plan',
    notGenerated: 'Not generated',
    planDetails: 'Plan Details',
    backupRecords: 'Backup Records',
    instanceRequired: 'Instance required',
    availableBackups: 'Available Backups',
    rollbackHint: 'Rollback restores captured files',
    instanceScan: 'Instance Scan',
    instanceManifest: 'Instance Manifest',
    waiting: 'Waiting',
    waitingInstance: 'Waiting for instance',
    readyBackupList: 'Ready to list backups',
    scanInstance: 'Scan Instance',
    buildManifest: 'Build a local manifest',
    readBackups: 'Read .packdelta backup manifests',
    scanBothPacks: 'Scan both official packs and calculate file diff',
    buildPlanActions: 'Build protected update actions',
    backupThenWrite: 'Backup first, then write changes',
    portableUpdate: 'Portable App Update',
    currentVersion: 'Current',
    releaseSource: 'Release Source',
    githubReleases: 'GitHub Releases',
    updateSourceHint: 'Portable updates use the configured GitHub release-index.json.',
    verified: 'Verified',
    available: 'Available',
    sourceDiffEmpty: 'No source diff loaded',
    sourceDiffHint: 'Select the old and new official pack folders, then compare sources.',
    noFilesGroup: 'No files in this group',
    moreFiles: 'more files',
    protectedUserFiles: 'Protected User Files',
    renamedFiles: 'Renamed',
    lastBackup: 'Last Backup',
    packName: 'Pack Name',
    packId: 'Pack ID',
    version: 'Version',
    files: 'Files',
    fileSummaryHint: 'Scan the instance to inspect file ownership and strategies.',
  },
} satisfies Record<Locale, Record<string, string>>

function isLocale(value: unknown): value is Locale {
  return value === 'zh' || value === 'en'
}

function readCachedPreferences(): AppPreferences | null {
  try {
    const raw = window.localStorage.getItem(PREFERENCES_STORAGE_KEY)
    return raw ? JSON.parse(raw) as AppPreferences : null
  } catch {
    return null
  }
}

function cachePreferences(preferences: AppPreferences) {
  try {
    window.localStorage.setItem(PREFERENCES_STORAGE_KEY, JSON.stringify(preferences))
  } catch {
    // OS-level preference storage is the source of truth in the desktop app.
  }
}

function restoredPreferenceCount(preferences: AppPreferences | null) {
  return [preferences?.instance_dir, preferences?.old_source, preferences?.new_source]
    .filter(Boolean)
    .length
}

function preferenceRestoreMessage(preferences: AppPreferences | null) {
  const count = restoredPreferenceCount(preferences)
  if (!count) {
    return '请选择实例目录、基准包和目标包，然后比较生成受保护的更新计划。'
  }
  return isLocale(preferences?.locale) && preferences.locale === 'en'
    ? `Restored ${count} previously selected directories.`
    : `已恢复上次选择的 ${count} 个目录。`
}

function App() {
  const initialPreferences = useMemo(() => readCachedPreferences(), [])
  const [activePage, setActivePage] = useState<ActivePage>('dashboard')
  const [locale, setLocale] = useState<Locale>(isLocale(initialPreferences?.locale) ? initialPreferences.locale : 'zh')
  const [instanceDir, setInstanceDir] = useState(initialPreferences?.instance_dir ?? '')
  const [oldSource, setOldSource] = useState(initialPreferences?.old_source ?? '')
  const [newSource, setNewSource] = useState(initialPreferences?.new_source ?? '')
  const [plan, setPlan] = useState<UpdatePlan | null>(null)
  const [backups, setBackups] = useState<BackupSummary[]>([])
  const [busy, setBusy] = useState('')
  const [message, setMessage] = useState(() => preferenceRestoreMessage(initialPreferences))
  const [lastApply, setLastApply] = useState<ApplyResult | null>(null)
  const [compareResult, setCompareResult] = useState<CompareResult | null>(null)
  const [instanceManifest, setInstanceManifest] = useState<PackManifest | null>(null)
  const [updateSource, setUpdateSource] = useState('')
  const [appUpdate, setAppUpdate] = useState<AppUpdateCheck | null>(null)
  const [downloadedUpdate, setDownloadedUpdate] = useState<DownloadedUpdate | null>(null)
  const [changelog, setChangelog] = useState<ChangelogRelease[]>([])
  const [appVersion, setAppVersion] = useState('0.1.0')
  const copy = i18n[locale]

  const canPreview = Boolean(instanceDir && oldSource && newSource && !busy)

  const totals = useMemo(() => {
    const p = plan
    return {
      changed: p ? p.added.length + p.removed.length + p.updated.length + p.merged.length + p.renamed.length : 0,
      added: p?.added.length ?? 0,
      updated: p ? p.updated.length + p.merged.length + p.renamed.length : 0,
      removed: p?.removed.length ?? 0,
      protected: p?.preserved.length ?? 0,
      conflicts: p?.conflicts.length ?? 0,
      backups: p?.backup_candidates.length ?? 0,
    }
  }, [plan])

  function buildPreferences(overrides: Partial<AppPreferences> = {}): AppPreferences {
    return {
      instance_dir: instanceDir || null,
      old_source: oldSource || null,
      new_source: newSource || null,
      locale,
      ...overrides,
    }
  }

  function persistPreferences(overrides: Partial<AppPreferences> = {}) {
    const preferences = buildPreferences(overrides)
    cachePreferences(preferences)
    invoke<AppPreferences>('save_app_preferences', { preferences })
      .then(cachePreferences)
      .catch(() => {
        // Selection still remains active in the current session.
      })
  }

  function changeLocale(nextLocale: Locale) {
    setLocale(nextLocale)
    persistPreferences({ locale: nextLocale })
  }

  useEffect(() => {
    getVersion()
      .then(setAppVersion)
      .catch(() => {
        // Browser-only previews cannot read Tauri metadata.
      })

    invoke<UpdateSourceConfig | null>('load_update_source')
      .then((config) => {
        if (config?.index_url) {
          setUpdateSource(config.index_url)
        }
      })
      .catch(() => {
        // Loading update settings is optional; manual input remains available.
      })

    invoke<AppPreferences>('load_app_preferences')
      .then((preferences) => {
        cachePreferences(preferences)
        if (isLocale(preferences.locale)) {
          setLocale(preferences.locale)
        }
        if (preferences.instance_dir) {
          setInstanceDir(preferences.instance_dir)
        }
        if (preferences.old_source) {
          setOldSource(preferences.old_source)
        }
        if (preferences.new_source) {
          setNewSource(preferences.new_source)
        }
        if (restoredPreferenceCount(preferences) > 0) {
          setMessage(preferenceRestoreMessage(preferences))
        }
      })
      .catch(() => {
        // Browser-only previews keep using localStorage preferences.
      })
  }, [])

  const pageCopy = {
    dashboard: {
      eyebrow: copy.packState,
      title: copy.dashboardTitle,
      body: message,
    },
    updates: {
      eyebrow: locale === 'zh' ? '应用分发' : 'Application Delivery',
      title: copy.updateTitle,
      body: message,
    },
    diff: {
      eyebrow: locale === 'zh' ? '整合包比较' : 'Pack Comparison',
      title: copy.diffTitle,
      body: compareResult
        ? locale === 'zh'
          ? `已比较 ${compareResult.old_manifest.files.length} 个基准文件和 ${compareResult.new_manifest.files.length} 个目标文件。`
          : `Compared ${compareResult.old_manifest.files.length} baseline files with ${compareResult.new_manifest.files.length} target files.`
        : locale === 'zh'
          ? '选择旧官方包和新官方包目录，然后比较源包。'
          : 'Select the old and new official pack folders, then run Compare Sources.',
    },
    plan: {
      eyebrow: locale === 'zh' ? '受保护更新' : 'Protected Update',
      title: copy.planTitle,
      body: plan
        ? locale === 'zh'
          ? `计划 ${plan.from} -> ${plan.to}: ${totals.changed} 个变更文件，${totals.conflicts} 个冲突。`
          : `Plan ${plan.from} -> ${plan.to}: ${totals.changed} changed files, ${totals.conflicts} conflicts.`
        : locale === 'zh'
          ? '选择三个目录后生成受保护的更新计划。'
          : 'Select all three folders, then generate the protected update plan.',
    },
    backups: {
      eyebrow: locale === 'zh' ? '恢复' : 'Recovery',
      title: copy.backupsTitle,
      body: backups.length
        ? locale === 'zh' ? `找到 ${backups.length} 条备份记录。` : `Found ${backups.length} backup records for this instance.`
        : locale === 'zh' ? '选择实例目录以读取备份记录。' : 'Select an instance directory to list backup records.',
    },
    instance: {
      eyebrow: locale === 'zh' ? '本地状态' : 'Local State',
      title: copy.instanceTitle,
      body: instanceManifest
        ? locale === 'zh' ? `已从 ${instanceManifest.pack_name} 扫描 ${instanceManifest.files.length} 个文件。` : `Scanned ${instanceManifest.files.length} files from ${instanceManifest.pack_name}.`
        : locale === 'zh' ? '选择并扫描 Minecraft 实例目录。' : 'Select and scan a Minecraft instance directory.',
    },
    changelog: {
      eyebrow: locale === 'zh' ? '发布历史' : 'Release History',
      title: copy.changelogTitle,
      body: changelog.length
        ? locale === 'zh' ? `已加载 ${changelog.length} 条 GitHub 发布日志。` : `Loaded ${changelog.length} GitHub release notes.`
        : message,
    },
  }[activePage]

  async function pickDirectory(key: PreferencePathKey, setter: (path: string) => void) {
    const selected = await open({ directory: true, multiple: false })
    if (typeof selected === 'string') {
      setter(selected)
      persistPreferences({ [key]: selected })
      setPlan(null)
      setLastApply(null)
      setCompareResult(null)
      setInstanceManifest(null)
    }
  }

  async function pickZipSource(key: PreferencePathKey, setter: (path: string) => void) {
    const selected = await open({
      directory: false,
      multiple: false,
      filters: [{ name: 'Minecraft pack zip', extensions: ['zip'] }],
    })
    if (typeof selected === 'string') {
      setter(selected)
      persistPreferences({ [key]: selected })
      setPlan(null)
      setLastApply(null)
      setCompareResult(null)
      setInstanceManifest(null)
    }
  }

  async function runCompareSources() {
    if (!oldSource || !newSource) return
    setBusy('source-compare')
    setMessage(locale === 'zh' ? '正在比较官方包源...' : 'Comparing official pack sources...')
    try {
      const result = await invoke<CompareResult>('compare_pack_sources', {
        oldSource,
        newSource,
      })
      setCompareResult(result)
      setMessage(
        locale === 'zh'
          ? `源包差异完成：新增 ${result.diff.added.length}，更新 ${result.diff.updated.length}，删除 ${result.diff.removed.length}。`
          : `Source diff complete: ${result.diff.added.length} added, ${result.diff.updated.length} updated, ${result.diff.removed.length} removed.`,
      )
    } catch (error) {
      setMessage(String(error))
    } finally {
      setBusy('')
    }
  }

  async function scanInstance() {
    if (!instanceDir) return
    setBusy('instance-scan')
    setMessage(locale === 'zh' ? '正在扫描选中的 Minecraft 实例...' : 'Scanning selected Minecraft instance...')
    try {
      const result = await invoke<PackManifest>('scan_pack_source', {
        path: instanceDir,
        options: null,
      })
      setInstanceManifest(result)
      setMessage(locale === 'zh' ? `实例扫描完成：${result.files.length} 个文件。` : `Instance scan complete: ${result.files.length} files.`)
      await refreshBackups()
    } catch (error) {
      setMessage(String(error))
    } finally {
      setBusy('')
    }
  }

  async function runPreview() {
    setBusy('compare')
    setMessage(locale === 'zh' ? '正在扫描官方源并生成安全的文件级更新计划...' : 'Scanning official sources and building a safe file-level update plan...')
    try {
      const result = await invoke<UpdatePlan>('preview_update', {
        instanceDir,
        oldSource,
        newSource,
      })
      setPlan(result)
      setMessage(
        locale === 'zh'
          ? `计划已生成：新增 ${result.added.length}，更新 ${result.updated.length}，合并配置 ${result.merged.length}，删除 ${result.removed.length}，冲突 ${result.conflicts.length}。`
          : `Plan generated: ${result.added.length} added, ${result.updated.length} updated, ${result.merged.length} config merges, ${result.removed.length} removed, ${result.conflicts.length} conflicts.`,
      )
      await refreshBackups()
    } catch (error) {
      setMessage(String(error))
    } finally {
      setBusy('')
    }
  }

  async function runApply() {
    setBusy('apply')
    setMessage(locale === 'zh' ? '正在创建备份并应用受保护的更新计划...' : 'Creating backup and applying the protected update plan...')
    try {
      const result = await invoke<ApplyResult>('apply_update', {
        instanceDir,
        oldSource,
        newSource,
      })
      setLastApply(result)
      setPlan(result.plan)
      setMessage(locale === 'zh' ? `更新完成。备份 ID：${result.backup_id}。冲突：${result.plan.conflicts.length}。` : `Update complete. Backup ID: ${result.backup_id}. Conflicts: ${result.plan.conflicts.length}.`)
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
      setMessage(
        result.length
          ? locale === 'zh' ? `找到 ${result.length} 条备份记录。` : `Found ${result.length} backup records for this instance.`
          : locale === 'zh' ? '此实例没有备份记录。' : 'No backup records found for this instance.',
      )
    } catch (error) {
      setMessage(String(error))
    } finally {
      setBusy('')
    }
  }

  async function rollback(backupId: string) {
    setBusy(`rollback:${backupId}`)
    setMessage(locale === 'zh' ? `正在还原备份 ${backupId}...` : `Rolling back backup ${backupId}...`)
    try {
      const result = await invoke<{ restored_files: number }>('rollback', {
        instanceDir,
        backupId,
      })
      setMessage(locale === 'zh' ? `还原完成。已恢复 ${result.restored_files} 个文件。` : `Rollback complete. Restored ${result.restored_files} files.`)
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
      setMessage(locale === 'zh' ? '已打开 .packdelta 状态目录。' : 'Opened the .packdelta state directory.')
    } catch (error) {
      setMessage(String(error))
    } finally {
      setBusy('')
    }
  }

  async function openConflictFolder() {
    if (!instanceDir || !plan || !lastApply) return
    setBusy('conflicts')
    try {
      await invoke('open_folder', { path: `${instanceDir}\\.packdelta\\conflicts\\${plan.from}_to_${plan.to}` })
      setMessage(locale === 'zh' ? '已打开冲突候选目录。' : 'Opened the conflict candidate directory.')
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
      setMessage(locale === 'zh' ? '应用更新源已保存。' : 'Application update source saved.')
    } catch (error) {
      setMessage(String(error))
    } finally {
      setBusy('')
    }
  }

  async function checkAppUpdate() {
    setBusy('check-app-update')
    setMessage(locale === 'zh' ? '正在检查 Kairos Patch 的 GitHub 更新索引...' : 'Checking GitHub release index for Kairos Patch updates...')
    try {
      const result = await invoke<AppUpdateCheck>('check_app_update', { indexUrl: updateSource })
      setAppUpdate(result)
      setDownloadedUpdate(null)
      await fetchChangelog(false)
      setMessage(
        result.update_available
          ? locale === 'zh' ? `Kairos Patch ${result.latest_version} 可用。当前版本：${result.current_version}。` : `Kairos Patch ${result.latest_version} is available. Current version: ${result.current_version}.`
          : locale === 'zh' ? `Kairos Patch 已是最新：${result.current_version}。` : `Kairos Patch is up to date: ${result.current_version}.`,
      )
    } catch (error) {
      setMessage(String(error))
    } finally {
      setBusy('')
    }
  }

  async function fetchChangelog(showMessage = true) {
    setBusy('changelog')
    if (showMessage) {
      setMessage(locale === 'zh' ? '正在加载 GitHub 发布日志...' : 'Loading GitHub release changelog...')
    }
    try {
      const result = await invoke<ChangelogRelease[]>('fetch_changelog', { indexUrl: updateSource })
      setChangelog(result)
      if (showMessage) {
        setMessage(result.length ? (locale === 'zh' ? `已加载 ${result.length} 条 GitHub 发布日志。` : `Loaded ${result.length} GitHub release notes.`) : (locale === 'zh' ? '没有找到 GitHub 发布日志。' : 'No GitHub release notes found.'))
      }
    } catch (error) {
      setMessage(String(error))
    } finally {
      setBusy('')
    }
  }

  async function downloadAppUpdate() {
    if (!appUpdate?.release) return
    setBusy('download-app-update')
    setMessage(locale === 'zh' ? `正在下载 Kairos Patch ${appUpdate.release.version} 便携包...` : `Downloading Kairos Patch ${appUpdate.release.version} portable package...`)
    try {
      const result = await invoke<DownloadedUpdate>('download_app_update', {
        release: appUpdate.release,
      })
      setDownloadedUpdate(result)
      setMessage(locale === 'zh' ? `便携更新包已下载并校验：${result.archive_path}` : `Portable update downloaded and verified: ${result.archive_path}`)
    } catch (error) {
      setMessage(String(error))
    } finally {
      setBusy('')
    }
  }

  async function installPortableUpdate() {
    if (!downloadedUpdate) return
    setBusy('install-app-update')
    setMessage(locale === 'zh' ? 'Kairos Patch 将关闭、替换便携文件并重启。' : 'Kairos Patch will close, replace the portable files, and restart.')
    try {
      await invoke('install_portable_update', { downloaded: downloadedUpdate })
    } catch (error) {
      setMessage(String(error))
      setBusy('')
    }
  }

  return (
    <main className="app-shell" data-locale={locale}>
      <section className="layout">
        <aside className="sidebar">
          <div className="brand-block">
            <div className="logo-cube">
              <Box size={28} />
            </div>
            <div>
              <strong>Kairos Patch</strong>
              <small>v{appVersion}</small>
            </div>
          </div>

          <nav className="nav-list" aria-label="Primary">
            <button type="button" className={activePage === 'dashboard' ? 'active' : ''} onClick={() => setActivePage('dashboard')}>
              <CircleDot size={16} /> {copy.dashboard}
            </button>
            <button type="button" className={activePage === 'diff' ? 'active' : ''} onClick={() => setActivePage('diff')}>
              <GitCompare size={16} /> {copy.diff}
            </button>
            <button type="button" className={activePage === 'plan' ? 'active' : ''} onClick={() => setActivePage('plan')}>
              <FileArchive size={16} /> {copy.plan}
            </button>
            <button type="button" className={activePage === 'backups' ? 'active' : ''} onClick={() => setActivePage('backups')}>
              <ArchiveRestore size={16} /> {copy.backups}
            </button>
            <button type="button" className={activePage === 'instance' ? 'active' : ''} onClick={() => setActivePage('instance')}>
              <HardDrive size={16} /> {copy.instance}
            </button>
            <button type="button" className={activePage === 'updates' ? 'active' : ''} onClick={() => setActivePage('updates')}>
              <RefreshCw size={16} /> {copy.appUpdate}
            </button>
            <button type="button" className={activePage === 'changelog' ? 'active' : ''} onClick={() => setActivePage('changelog')}>
              <History size={16} /> {copy.changelog}
            </button>
          </nav>

          <div className="language-switch" aria-label="Language">
            <button type="button" className={locale === 'zh' ? 'active' : ''} onClick={() => changeLocale('zh')}>中文</button>
            <button type="button" className={locale === 'en' ? 'active' : ''} onClick={() => changeLocale('en')}>EN</button>
          </div>

          <div className="sidebar-card">
            <div className="pack-icon">
              <Box size={22} />
            </div>
            <div>
              <strong>{copy.selectedPack}</strong>
              <span>{plan ? `${plan.from} -> ${plan.to}` : copy.noPlan}</span>
            </div>
            <small className="ready-dot">{plan ? copy.planned : copy.idle}</small>
          </div>
        </aside>

        <section className="workspace">
          <header className="hero-header">
            <div>
              <p className="eyebrow">{pageCopy.eyebrow}</p>
              <h1>{pageCopy.title}</h1>
              <span>{pageCopy.body}</span>
            </div>
            <div className={`status-pill ${busy ? 'busy' : ''}`}>
              <span />
              {busy ? copy.working : copy.ready}
            </div>
          </header>

          {activePage === 'dashboard' && (
            <>
          <section className="source-grid" aria-label="Pack sources">
            <SourceCard
              icon={<HardDrive size={19} />}
              title={copy.instanceDir}
              badge={copy.inUse}
              badgeTone="green"
              value={instanceDir}
              fallback={copy.selectInstance}
              version={plan ? `${locale === 'zh' ? '实例基准' : 'Instance baseline'} ${plan.from}` : copy.waitingScan}
              meta={instanceDir ? copy.selected : copy.required}
              onPick={() => pickDirectory('instance_dir', setInstanceDir)}
            />
            <SourceCard
              icon={<FolderOpen size={19} />}
              title={copy.oldPack}
              badge={copy.baseline}
              badgeTone="amber"
              value={oldSource}
              fallback={copy.selectOld}
              version={plan ? `${locale === 'zh' ? '当前基准' : 'Current baseline'} ${plan.from}` : copy.notScanned}
              meta={oldSource ? copy.selected : copy.required}
              onPick={() => pickDirectory('old_source', setOldSource)}
              onPickZip={() => pickZipSource('old_source', setOldSource)}
            />
            <SourceCard
              icon={<Sparkles size={19} />}
              title={copy.newPack}
              badge={copy.target}
              badgeTone="violet"
              value={newSource}
              fallback={copy.selectNew}
              version={plan ? `${locale === 'zh' ? '目标版本' : 'Target version'} ${plan.to}` : copy.notScanned}
              meta={newSource ? copy.selected : copy.required}
              onPick={() => pickDirectory('new_source', setNewSource)}
              onPickZip={() => pickZipSource('new_source', setNewSource)}
            />
          </section>

          <section className="action-row">
            <button type="button" className="action-button primary" onClick={runPreview} disabled={!canPreview}>
              <GitCompare size={18} />
              <span>{copy.compare}</span>
              <small>{copy.generatePlan}</small>
            </button>
            <button type="button" className="action-button" onClick={runApply} disabled={!canPreview || !plan}>
              <FileArchive size={18} />
              <span>{copy.createBackup}</span>
              <small>{locale === 'zh' ? '保护已修改文件' : 'Protect modified files'}</small>
            </button>
            <button type="button" className="action-button" onClick={openPackDelta} disabled={!instanceDir || Boolean(busy)}>
              <Folder size={18} />
              <span>{copy.openPackdelta}</span>
              <small>{locale === 'zh' ? '打开状态目录' : 'Open state folder'}</small>
            </button>
            <button type="button" className="action-button compact" onClick={refreshBackups} disabled={!instanceDir || Boolean(busy)}>
              <RefreshCw size={18} />
              <span>{copy.refreshScan}</span>
            </button>
          </section>

          <section className="stats-grid" aria-label="Update stats">
            <Metric icon={<FileWarning size={22} />} label={copy.pendingChanges} value={totals.changed} tone="blue" />
            <Metric icon={<FilePlus2 size={22} />} label={copy.addedFiles} value={totals.added} tone="green" />
            <Metric icon={<RotateCcw size={22} />} label={copy.updatedFiles} value={totals.updated} tone="amber" />
            <Metric icon={<Trash2 size={22} />} label={copy.removedFiles} value={totals.removed} tone="red" />
            <Metric icon={<AlertTriangle size={22} />} label={copy.conflicts} value={totals.conflicts} tone="violet" />
          </section>

          <section className="content-grid">
            <section className="panel diff-panel" id="diff">
              <PanelHeader title={copy.diffPreview} subtitle={plan ? `${plan.from} -> ${plan.to}` : copy.runCompareHint} />
              <DiffView plan={plan} copy={copy} />
            </section>

            <section className="panel plan-panel" id="plan">
              <PanelHeader title={copy.updatePlan} subtitle={copy.estimated} />
              <Stepper conflicts={totals.conflicts} hasPlan={Boolean(plan)} locale={locale} />
            </section>

            <aside className="right-stack">
              <section className="panel side-panel" id="protection">
                <div className="side-title">
                  <span className="side-icon danger"><AlertTriangle size={22} /></span>
                  <div>
                    <h2>{copy.conflictProtection}</h2>
                    <p>{plan ? `${totals.conflicts} ${copy.conflicts}, ${totals.protected} ${locale === 'zh' ? '个保留文件' : 'preserved files'}` : copy.checkConflicts}</p>
                  </div>
                </div>
                <button type="button" className="ghost-button" onClick={openConflictFolder} disabled={!lastApply || !plan || !plan.conflicts.length || Boolean(busy)}>{locale === 'zh' ? '审阅' : 'Review'}</button>
              </section>

              <section className="panel side-panel" id="backups">
                <div className="side-title">
                  <span className="side-icon success"><ArchiveRestore size={22} /></span>
                  <div>
                    <h2>{copy.latestBackup}</h2>
                    {backups.length ? (
                      <p>{backups[0].from} {'->'} {backups[0].to} · {backups[0].file_count} {copy.files}</p>
                    ) : (
                      <p>{copy.noBackup}</p>
                    )}
                  </div>
                </div>
                {backups[0] ? (
                  <button type="button" className="ghost-button" onClick={() => rollback(backups[0].id)} disabled={Boolean(busy)}>
                    {copy.restore}
                  </button>
                ) : (
                  <button type="button" className="ghost-button" onClick={refreshBackups} disabled={!instanceDir || Boolean(busy)}>
                    {copy.check}
                  </button>
                )}
              </section>

              <section className="panel instance-panel" id="instance">
                <PanelHeader title={copy.instanceInfo} />
                <InfoRow label={copy.instance} value={instanceDir || (locale === 'zh' ? '未选择' : 'Not selected')} />
                <InfoRow label={locale === 'zh' ? '旧源包' : 'Old Source'} value={oldSource || (locale === 'zh' ? '未选择' : 'Not selected')} />
                <InfoRow label={locale === 'zh' ? '新源包' : 'New Source'} value={newSource || (locale === 'zh' ? '未选择' : 'Not selected')} />
                <InfoRow label={locale === 'zh' ? '当前' : 'Current'} value={plan?.from ?? copy.notScanned} />
                <InfoRow label={copy.target} value={plan?.to ?? copy.notScanned} />
                {lastApply && <InfoRow label={copy.lastBackup} value={lastApply.backup_id} />}
              </section>

            </aside>
          </section>

          <footer className="execute-bar">
            <button type="button" className="execute-button" onClick={runApply} disabled={!canPreview || !plan}>
              <Rocket size={20} />
              <span>{copy.executePlan}</span>
              <small>{copy.backupFirst}</small>
            </button>
            <div className="safety-note">
              <ShieldCheck size={18} />
              {copy.backupFirst}
            </div>
          </footer>
            </>
          )}

          {activePage === 'diff' && (
            <section className="single-page-grid">
              <section className="panel workflow-panel">
                <PanelHeader title={copy.sourceSelection} subtitle={copy.oldVsNew} />
                <div className="page-source-stack">
                  <SourceCard
                    icon={<FolderOpen size={19} />}
                    title={copy.oldPack}
                    badge={copy.baseline}
                    badgeTone="amber"
                    value={oldSource}
                    fallback={copy.selectOld}
                    version={compareResult ? compareResult.old_manifest.version : copy.notScanned}
                    meta={oldSource ? copy.selected : copy.required}
                    onPick={() => pickDirectory('old_source', setOldSource)}
                    onPickZip={() => pickZipSource('old_source', setOldSource)}
                  />
                  <SourceCard
                    icon={<Sparkles size={19} />}
                    title={copy.newPack}
                    badge={copy.target}
                    badgeTone="violet"
                    value={newSource}
                    fallback={copy.selectNew}
                    version={compareResult ? compareResult.new_manifest.version : copy.notScanned}
                    meta={newSource ? copy.selected : copy.required}
                    onPick={() => pickDirectory('new_source', setNewSource)}
                    onPickZip={() => pickZipSource('new_source', setNewSource)}
                  />
                </div>
                <button type="button" className="action-button primary page-action" onClick={runCompareSources} disabled={!oldSource || !newSource || Boolean(busy)}>
                  <GitCompare size={18} />
                  <span>{copy.compareSources}</span>
                  <small>{copy.scanBothPacks}</small>
                </button>
              </section>
              <section className="panel diff-panel">
                <PanelHeader
                  title={copy.diffResult}
                  subtitle={compareResult ? `${compareResult.old_manifest.pack_name} -> ${compareResult.new_manifest.pack_name}` : copy.waitingCompare}
                />
                <DiffResultView compareResult={compareResult} copy={copy} />
              </section>
            </section>
          )}

          {activePage === 'plan' && (
            <section className="single-page-grid">
              <section className="panel workflow-panel">
                <PanelHeader title={copy.protectedPlan} subtitle={plan ? `${plan.from} -> ${plan.to}` : copy.notGenerated} />
                <div className="page-source-stack">
                  <SourceCard
                    icon={<HardDrive size={19} />}
                    title={copy.instanceDir}
                    badge={copy.inUse}
                    badgeTone="green"
                    value={instanceDir}
                    fallback={copy.selectInstance}
                    version={plan ? `${locale === 'zh' ? '实例基准' : 'Instance baseline'} ${plan.from}` : copy.waitingScan}
                    meta={instanceDir ? copy.selected : copy.required}
                    onPick={() => pickDirectory('instance_dir', setInstanceDir)}
                  />
                  <SourceCard
                    icon={<FolderOpen size={19} />}
                    title={copy.oldPack}
                    badge={copy.baseline}
                    badgeTone="amber"
                    value={oldSource}
                    fallback={copy.selectOld}
                    version={plan ? `${locale === 'zh' ? '当前基准' : 'Current baseline'} ${plan.from}` : copy.notScanned}
                    meta={oldSource ? copy.selected : copy.required}
                    onPick={() => pickDirectory('old_source', setOldSource)}
                    onPickZip={() => pickZipSource('old_source', setOldSource)}
                  />
                  <SourceCard
                    icon={<Sparkles size={19} />}
                    title={copy.newPack}
                    badge={copy.target}
                    badgeTone="violet"
                    value={newSource}
                    fallback={copy.selectNew}
                    version={plan ? `${locale === 'zh' ? '目标版本' : 'Target version'} ${plan.to}` : copy.notScanned}
                    meta={newSource ? copy.selected : copy.required}
                    onPick={() => pickDirectory('new_source', setNewSource)}
                    onPickZip={() => pickZipSource('new_source', setNewSource)}
                  />
                </div>
                <div className="page-button-row">
                  <button type="button" className="action-button primary" onClick={runPreview} disabled={!canPreview}>
                    <GitCompare size={18} />
                    <span>{copy.generatePlan}</span>
                    <small>{copy.buildPlanActions}</small>
                  </button>
                  <button type="button" className="action-button" onClick={runApply} disabled={!canPreview || !plan}>
                    <Rocket size={18} />
                    <span>{copy.executePlan}</span>
                    <small>{copy.backupThenWrite}</small>
                  </button>
                </div>
              </section>
              <section className="panel plan-panel">
                <PanelHeader title={copy.planDetails} subtitle={locale === 'zh' ? `${totals.changed} 个变更，${totals.conflicts} 个冲突` : `${totals.changed} changed, ${totals.conflicts} conflicts`} />
                <Stepper conflicts={totals.conflicts} hasPlan={Boolean(plan)} locale={locale} />
                <ConflictList plan={plan} locale={locale} />
              </section>
            </section>
          )}

          {activePage === 'backups' && (
            <section className="single-page-grid">
              <section className="panel workflow-panel">
                <PanelHeader title={copy.backupRecords} subtitle={backups.length ? (locale === 'zh' ? `找到 ${backups.length} 条` : `${backups.length} found`) : copy.instanceRequired} />
                <SourceCard
                  icon={<HardDrive size={19} />}
                  title={copy.instanceDir}
                  badge={copy.inUse}
                  badgeTone="green"
                  value={instanceDir}
                  fallback={copy.selectInstance}
                  version={instanceDir ? copy.selected : copy.waitingInstance}
                  meta={instanceDir ? copy.readyBackupList : copy.required}
                  onPick={() => pickDirectory('instance_dir', setInstanceDir)}
                />
                <button type="button" className="action-button primary page-action" onClick={refreshBackups} disabled={!instanceDir || Boolean(busy)}>
                  <RefreshCw size={18} />
                  <span>{copy.refreshScan}</span>
                  <small>{copy.readBackups}</small>
                </button>
              </section>
              <section className="panel backup-list-panel">
                <PanelHeader title={copy.availableBackups} subtitle={copy.rollbackHint} />
                <BackupList backups={backups} busy={busy} onRollback={rollback} copy={copy} />
              </section>
            </section>
          )}

          {activePage === 'instance' && (
            <section className="single-page-grid">
              <section className="panel workflow-panel">
                <PanelHeader title={copy.instanceScan} subtitle={instanceManifest ? `${instanceManifest.files.length} ${copy.files}` : copy.notScanned} />
                <SourceCard
                  icon={<HardDrive size={19} />}
                  title={copy.instanceDir}
                  badge={copy.inUse}
                  badgeTone="green"
                  value={instanceDir}
                  fallback={copy.selectInstance}
                  version={instanceManifest?.pack_name ?? copy.waitingScan}
                  meta={instanceDir ? copy.selected : copy.required}
                  onPick={() => pickDirectory('instance_dir', setInstanceDir)}
                />
                <div className="page-button-row">
                  <button type="button" className="action-button primary" onClick={scanInstance} disabled={!instanceDir || Boolean(busy)}>
                    <HardDrive size={18} />
                    <span>{copy.scanInstance}</span>
                    <small>{copy.buildManifest}</small>
                  </button>
                  <button type="button" className="action-button" onClick={openPackDelta} disabled={!instanceDir || Boolean(busy)}>
                    <Folder size={18} />
                    <span>{copy.openPackdelta}</span>
                    <small>{locale === 'zh' ? '打开状态目录' : 'Open state folder'}</small>
                  </button>
                </div>
              </section>
              <section className="panel instance-panel">
                <PanelHeader title={copy.instanceManifest} subtitle={instanceManifest?.created_at ? new Date(instanceManifest.created_at).toLocaleString() : copy.waiting} />
                <InfoRow label={copy.instance} value={instanceDir || (locale === 'zh' ? '未选择' : 'Not selected')} />
                <InfoRow label={copy.packName} value={instanceManifest?.pack_name ?? copy.notScanned} />
                <InfoRow label={copy.packId} value={instanceManifest?.pack_id ?? copy.notScanned} />
                <InfoRow label={copy.version} value={instanceManifest?.version ?? copy.notScanned} />
                <InfoRow label={copy.files} value={String(instanceManifest?.files.length ?? 0)} />
                <FileTypeSummary manifest={instanceManifest} copy={copy} />
              </section>
            </section>
          )}

          {activePage === 'updates' && (
            <section className="single-page-grid">
              <section className="panel app-update-panel app-update-page">
                <PanelHeader
                  title={copy.portableUpdate}
                  subtitle={appUpdate ? `${appUpdate.current_version} -> ${appUpdate.latest_version}` : `${copy.currentVersion} ${appVersion}`}
                />
                <input
                  className="update-source-input"
                  value={updateSource}
                  onChange={(event) => setUpdateSource(event.target.value)}
                  placeholder="https://github.com/SevenThRe/karios-patch/releases/latest/download/release-index.json"
                />
                <div className="update-controls">
                  <button type="button" className="ghost-button" onClick={saveUpdateSource} disabled={!updateSource || Boolean(busy)}>
                    {copy.save}
                  </button>
                  <button type="button" className="ghost-button" onClick={checkAppUpdate} disabled={!updateSource || Boolean(busy)}>
                    {copy.check}
                  </button>
                  <button type="button" className="ghost-button" onClick={() => fetchChangelog()} disabled={!updateSource || Boolean(busy)}>
                    {copy.notes}
                  </button>
                  <button type="button" className="ghost-button" onClick={downloadAppUpdate} disabled={!appUpdate?.release || Boolean(busy)}>
                    {copy.download}
                  </button>
                  <button type="button" className="ghost-button" onClick={installPortableUpdate} disabled={!downloadedUpdate || Boolean(busy)}>
                    {copy.apply}
                  </button>
                </div>
                <p className="update-status">
                  {downloadedUpdate
                    ? `${copy.verified} ${downloadedUpdate.version}: ${downloadedUpdate.archive_path}`
                    : appUpdate?.update_available
                      ? `${copy.available}: ${appUpdate.latest_version}`
                      : copy.updateSourceHint}
                </p>
              </section>

              <section className="panel release-summary-panel">
                <PanelHeader title={copy.releaseSource} subtitle="GitHub" />
                <InfoRow label={copy.appVersion} value={appVersion} />
                <InfoRow label={copy.latestChecked} value={appUpdate?.latest_version ?? copy.notChecked} />
                <InfoRow label={copy.downloaded} value={downloadedUpdate?.version ?? copy.none} />
                <InfoRow label={copy.updateSource} value={updateSource || copy.notConfigured} />
              </section>
            </section>
          )}

          {activePage === 'changelog' && (
            <section className="single-page-grid">
              <section className="panel changelog-panel changelog-page">
                <PanelHeader title={copy.changelogTitle} subtitle={copy.githubReleases} />
                <ChangelogView releases={changelog} onLoad={() => fetchChangelog()} disabled={!updateSource || Boolean(busy)} copy={copy} />
              </section>
            </section>
          )}
        </section>
      </section>
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
  onPickZip,
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
  onPickZip?: () => void
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
        <button
          type="button"
          onClick={onPickZip}
          disabled={!onPickZip}
          aria-label={onPickZip ? `Select ${title} ZIP` : 'More options'}
        >
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

function DiffView({ plan, copy }: { plan: UpdatePlan | null; copy: typeof i18n.zh }) {
  if (!plan) {
    return (
      <div className="empty-state">
        <GitCompare size={24} />
        <strong>{copy.noComparison}</strong>
        <p>{copy.noComparisonHint}</p>
      </div>
    )
  }

  const sections = [
    { tone: 'green', title: copy.addedFiles, count: plan.added.length, items: plan.added.map((item) => item.path) },
    { tone: 'amber', title: copy.updatedFiles, count: plan.updated.length + plan.renamed.length, items: [...plan.updated.map((item) => item.path), ...plan.renamed.map((item) => `${item.from} -> ${item.to}`)] },
    { tone: 'blue', title: copy.mergedConfigs, count: plan.merged.length, items: plan.merged.map((item) => item.path) },
    { tone: 'red', title: copy.removedFiles, count: plan.removed.length, items: plan.removed.map((item) => item.path) },
    { tone: 'violet', title: copy.protectedUserFiles, count: plan.preserved.length, items: plan.preserved },
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
            {section.count > 3 && <li>... {section.count - 3} {copy.moreFiles}</li>}
            {!section.items.length && <li>{copy.noFilesGroup}</li>}
          </ul>
        </section>
      ))}
    </div>
  )
}

function DiffResultView({ compareResult, copy }: { compareResult: CompareResult | null; copy: typeof i18n.zh }) {
  if (!compareResult) {
    return (
      <div className="empty-state">
        <GitCompare size={24} />
        <strong>{copy.sourceDiffEmpty}</strong>
        <p>{copy.sourceDiffHint}</p>
      </div>
    )
  }

  const diff = compareResult.diff
  const sections = [
    { tone: 'green', title: copy.addedFiles, count: diff.added.length, items: diff.added.map((item) => item.path) },
    { tone: 'amber', title: copy.updatedFiles, count: diff.updated.length, items: diff.updated.map((item) => item.new.path) },
    { tone: 'red', title: copy.removedFiles, count: diff.removed.length, items: diff.removed.map((item) => item.path) },
    { tone: 'violet', title: copy.renamedFiles, count: diff.renamed.length, items: diff.renamed.map((item) => `${item.old.path} -> ${item.new.path}`) },
  ]

  return (
    <div className="diff-result-layout">
      <div className="mini-metrics">
        <Metric icon={<FilePlus2 size={22} />} label={copy.addedFiles} value={diff.added.length} tone="green" />
        <Metric icon={<RotateCcw size={22} />} label={copy.updatedFiles} value={diff.updated.length + diff.renamed.length} tone="amber" />
        <Metric icon={<Trash2 size={22} />} label={copy.removedFiles} value={diff.removed.length} tone="red" />
      </div>
      <div className="diff-list">
        {sections.map((section) => (
          <section className={`diff-section ${section.tone}`} key={section.title}>
            <button type="button" className="diff-section-head">
              <span>{section.title} <strong>({section.count})</strong></span>
              <ChevronRight size={17} />
            </button>
            <ul>
              {section.items.slice(0, 8).map((item) => (
                <li key={item}>{item}</li>
              ))}
              {section.count > 8 && <li>... {section.count - 8} {copy.moreFiles}</li>}
              {!section.items.length && <li>{copy.noFilesGroup}</li>}
            </ul>
          </section>
        ))}
      </div>
    </div>
  )
}

function ConflictList({ plan, locale }: { plan: UpdatePlan | null; locale: Locale }) {
  if (!plan) {
    return (
      <div className="inline-empty">
        <AlertTriangle size={17} />
        {locale === 'zh' ? '生成计划后查看冲突和保留文件。' : 'Generate a plan to inspect conflicts and preserved files.'}
      </div>
    )
  }

  return (
    <div className="conflict-list">
      <section>
        <h3>{locale === 'zh' ? '冲突' : 'Conflicts'}</h3>
        <ul>
          {plan.conflicts.map((conflict) => (
            <li key={conflict.path}>
              <strong>{conflict.path}</strong>
              <span>{conflict.reason}</span>
            </li>
          ))}
          {!plan.conflicts.length && <li>{locale === 'zh' ? '未检测到阻塞冲突' : 'No blocking conflicts detected'}</li>}
        </ul>
      </section>
      <section>
        <h3>{locale === 'zh' ? '保留的用户文件' : 'Preserved User Files'}</h3>
        <ul>
          {plan.preserved.slice(0, 10).map((path) => (
            <li key={path}>{path}</li>
          ))}
          {plan.preserved.length > 10 && <li>... {plan.preserved.length - 10} {locale === 'zh' ? '个文件' : 'more files'}</li>}
          {!plan.preserved.length && <li>{locale === 'zh' ? '此计划中没有保留的用户文件' : 'No preserved user files in this plan'}</li>}
        </ul>
      </section>
    </div>
  )
}

function BackupList({
  backups,
  busy,
  onRollback,
  copy,
}: {
  backups: BackupSummary[]
  busy: string
  onRollback: (backupId: string) => void
  copy: typeof i18n.zh
}) {
  if (!backups.length) {
    return (
      <div className="empty-state compact-empty">
        <ArchiveRestore size={24} />
        <strong>{copy.noBackup}</strong>
        <p>{copy.selectInstance}</p>
      </div>
    )
  }

  return (
    <div className="backup-list">
      {backups.map((backup) => (
        <article className="backup-row" key={backup.id}>
          <div>
            <strong>{backup.from} {'->'} {backup.to}</strong>
            <span>{backup.id}</span>
          </div>
          <small>{backup.file_count} {copy.files}</small>
          <button type="button" className="ghost-button" onClick={() => onRollback(backup.id)} disabled={Boolean(busy)}>
            {copy.restore}
          </button>
        </article>
      ))}
    </div>
  )
}

function FileTypeSummary({ manifest, copy }: { manifest: PackManifest | null; copy: typeof i18n.zh }) {
  if (!manifest) {
    return (
      <div className="inline-empty">
        <HardDrive size={17} />
        {copy.fileSummaryHint}
      </div>
    )
  }

  const counts = manifest.files.reduce<Record<string, number>>((acc, file) => {
    acc[file.type] = (acc[file.type] ?? 0) + 1
    return acc
  }, {})

  return (
    <div className="file-type-summary">
      {Object.entries(counts).map(([type, count]) => (
        <InfoRow key={type} label={fileTypeLabel(type, copy)} value={String(count)} />
      ))}
    </div>
  )
}

function fileTypeLabel(type: string, copy: typeof i18n.zh) {
  if (copy.dashboard !== '仪表盘') {
    return type
  }
  const labels: Record<string, string> = {
    mod: '模组',
    config: '配置',
    script: '脚本',
    resourcepack: '资源包',
    shaderpack: '光影包',
    save: '存档',
    runtime: '运行时',
    other: '其他',
  }
  return labels[type.toLowerCase()] ?? type
}

function ChangelogView({
  releases,
  onLoad,
  disabled,
  copy,
}: {
  releases: ChangelogRelease[]
  onLoad: () => void
  disabled: boolean
  copy: typeof i18n.zh
}) {
  if (!releases.length) {
    return (
      <div className="empty-state compact-empty">
        <History size={24} />
        <strong>{copy.noChangelog}</strong>
        <p>{copy.noChangelogHint}</p>
        <button type="button" className="ghost-button" onClick={onLoad} disabled={disabled}>
          {copy.loadNotes}
        </button>
      </div>
    )
  }

  return (
    <div className="changelog-list">
      {releases.map((release) => (
        <article className="changelog-release" key={release.url}>
          <div className="changelog-head">
            <div>
              <strong>{release.title}</strong>
              <span>{release.version}</span>
            </div>
            <a href={release.url} target="_blank" rel="noreferrer" aria-label={`Open ${release.version} on GitHub`}>
              <ExternalLink size={16} />
            </a>
          </div>
          {release.published_at && (
            <small className="release-date">
              <CalendarDays size={14} />
              {new Date(release.published_at).toLocaleDateString()}
            </small>
          )}
          <p>{release.body}</p>
        </article>
      ))}
    </div>
  )
}

function Stepper({ conflicts, hasPlan, locale = 'en' }: { conflicts: number; hasPlan: boolean; locale?: Locale }) {
  const steps = [
    { title: locale === 'zh' ? '验证' : 'Verify', body: locale === 'zh' ? '检查源映射和文件完整性' : 'Check source maps and file integrity' },
    { title: locale === 'zh' ? '备份' : 'Backup', body: locale === 'zh' ? '复制可能被修改的文件' : 'Copy files that may be modified' },
    { title: locale === 'zh' ? '应用变更' : 'Apply Changes', body: locale === 'zh' ? '写入官方包变更' : 'Write official pack changes' },
    { title: locale === 'zh' ? '处理冲突' : 'Resolve Conflicts', body: conflicts ? (locale === 'zh' ? '需要手动审阅' : 'Manual review required') : (locale === 'zh' ? '未检测到阻塞冲突' : 'No blocking conflicts detected') },
    { title: locale === 'zh' ? '完成' : 'Finish', body: locale === 'zh' ? '写入状态并清理临时文件' : 'Write state and clean temporary files' },
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
