import { useEffect, useMemo, useRef, useState } from 'react'
import {
  ArchiveRestore,
  Bug,
  Download,
  ExternalLink,
  Folder,
  FolderOpen,
  GitCompare,
  HardDrive,
  History,
  MoreHorizontal,
  Paperclip,
  RefreshCw,
  Rocket,
  Settings,
  X,
} from 'lucide-react'
import { invoke } from '@tauri-apps/api/core'
import { getVersion } from '@tauri-apps/api/app'
import { listen } from '@tauri-apps/api/event'
import { open } from '@tauri-apps/plugin-dialog'
import { DiffEditor } from '@monaco-editor/react'
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

type OperationLog = {
  level: 'info' | 'warn' | 'error'
  message: string
}

type OperationProgress = {
  operation_id: string
  stage: string
  message: string
  current: number
  total: number
  percent: number
  path?: string | null
  done: boolean
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
  already_current: FileAction[]
  conflicts: Conflict[]
  backup_candidates: string[]
  logs: OperationLog[]
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
  created_at: string
  files: ManifestFile[]
}

type CompareResult = {
  old_manifest: PackManifest
  new_manifest: PackManifest
  diff: {
    from: string
    to: string
    added: ManifestFile[]
    removed: ManifestFile[]
    updated: Array<{ old: ManifestFile; new: ManifestFile }>
    renamed: Array<{ old: ManifestFile; new: ManifestFile }>
    unchanged: ManifestFile[]
  }
}

type BackupSummary = {
  id: string
  from: string
  to: string
  file_count: number
}

type ApplyResult = {
  backup_id?: string | null
  plan: UpdatePlan
  state_path: string
  logs: OperationLog[]
}

type ConservativeApplyResult = {
  backup_id?: string | null
  target_version: string
  applied_changes: Array<{
    path: string
    action: string
    source_path?: string | null
  }>
  preserved_paths: string[]
  protected_paths: string[]
  state_path: string
  logs: string[]
}

type AppPreferences = {
  instance_dir?: string | null
  old_source?: string | null
  new_source?: string | null
  locale?: Locale | null
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

type ChangelogRelease = {
  version: string
  title: string
  body: string
  published_at?: string
  url: string
}

type SourceDiffPreview = {
  path: string
  old_text: string
  new_text: string
  language: string
  notice?: string | null
}

type ConservativeAction = {
  path: string
  action: string
  reason: string
  mod_name?: string | null
  from_version?: string | null
  to_version?: string | null
}

type ReviewItem = {
  id: string
  path: string
  kind: string
  reason: string
  default_choice: string
  choices: string[]
  mod_name?: string | null
  mod_id?: string | null
  local_version?: string | null
  target_version?: string | null
}

type ProtectedItem = {
  path: string
  reason: string
}

type ConservativeUpdatePlan = {
  mode: string
  target_version: string
  auto_actions: ConservativeAction[]
  review_items: ReviewItem[]
  protected_items: ProtectedItem[]
  logs: string[]
}

type FeedbackPackage = {
  report_path: string
  archive_path: string
  issue_body_path: string
  app_log_path: string
  issue_url: string
}

type FeedbackIssueType = 'Bug' | '建议' | '安装失败' | '更新失败' | '其他'

type FeedbackFormState = {
  issueType: FeedbackIssueType
  title: string
  description: string
  reproductionSteps: string
  includeLogs: boolean
  includeConfig: boolean
  attachmentPaths: string[]
  contact: string
}

type DiffTone = 'added' | 'updated' | 'removed' | 'protected' | 'conflict'

type DiffFileCandidate = {
  path: string
  label: string
  tone: DiffTone
}

type ActivePage = 'update' | 'backups' | 'settings'
type Locale = 'zh' | 'en'
type PreferencePathKey = 'instance_dir' | 'old_source' | 'new_source'

const PREFERENCES_STORAGE_KEY = 'kairos-patch:preferences'
const feedbackIssueTypes: FeedbackIssueType[] = ['Bug', '建议', '安装失败', '更新失败', '其他']
const defaultFeedbackForm: FeedbackFormState = {
  issueType: 'Bug',
  title: '',
  description: '',
  reproductionSteps: '',
  includeLogs: true,
  includeConfig: false,
  attachmentPaths: [],
  contact: '',
}

const copy = {
  zh: {
    update: '更新',
    backups: '备份',
    settings: '设置',
    title: 'Kairos Patch',
    subtitle: 'Minecraft 整合包更新工具',
    instance: '当前实例',
    oldPack: '当前基线',
    oldPackAdvanced: '高级：当前基线包（可选）',
    newPack: '目标版本',
    selectInstance: '选择 Minecraft 实例目录',
    selectOld: '选择旧官方包目录或 ZIP',
    selectNew: '选择新官方包目录或 ZIP',
    dropPath: '拖拽到这里',
    dropPathUnsupported: '没有读取到拖拽路径，请使用选择按钮。',
    operationActive: '正在更新',
    operationDone: '更新完成',
    operationFailed: '更新失败',
    chooseFolder: '选择目录',
    chooseZip: '选择 ZIP',
    compare: '检查更新',
    apply: '开始更新',
    refresh: '刷新备份',
    openState: '打开 .packdelta',
    safe: '可安全更新',
    blocked: '需要确认冲突',
    waiting: '等待检查',
    missing: '路径未完整',
    working: '处理中',
    planTitle: '检测到更新',
    noPlan: '未检查',
    emptyPlanBody: '等待实例和目标版本。',
    safeBody: '更新项已准备。',
    conflictBody: '待确认项已列出。',
    autoActions: '可自动处理',
    reviewItems: '需要你确认',
    protectedItems: '始终保护',
    conservativeMode: '保守模式',
    changedFiles: '文件变化',
    protectedFiles: '将保留用户文件',
    conflicts: '需要确认',
    updated: '更新',
    added: '新增',
    removed: '移除',
    protected: '保护',
    viewDiff: '查看差异',
    diff: '差异',
    fullDiff: '完整文件差异',
    noDiff: '选择文件后显示内置 diff。',
    diffUnavailable: '这个文件暂时不能用文本方式预览。',
    recentBackups: '备份记录',
    noBackups: '还没有备份记录。',
    rollback: '还原',
    appUpdate: '应用更新',
    feedback: '反馈 Bug',
    createFeedback: '生成反馈包',
    feedbackBody: '填写反馈表单，生成本地诊断包，并打开 GitHub Issue 模板。',
    issueType: '问题类型',
    feedbackTitle: '标题',
    feedbackDescription: '描述',
    reproductionSteps: '复现步骤',
    includeLogs: '上传日志',
    includeConfig: '上传配置',
    attachments: '截图/附件',
    chooseAttachments: '选择附件',
    clearAttachments: '清空附件',
    contact: '联系方式（可选）',
    feedbackTitlePlaceholder: '例如：更新到 0.1.2 后启动失败',
    feedbackDescriptionPlaceholder: '说明你看到的现象、期望结果和实际结果。',
    reproductionStepsPlaceholder: '1. 打开工具\n2. 选择实例和目标包\n3. 点击开始更新',
    contactPlaceholder: '邮箱、GitHub ID 或其他联系方式',
    issueBody: 'Issue 内容',
    diagnosticPackage: '诊断包',
    appLogPath: '日志文件',
    updateSource: '更新源',
    save: '保存',
    check: '检查',
    download: '下载',
    install: '应用',
    changelog: '更新日志',
    loadNotes: '加载日志',
    language: '语言',
    lastLog: '最近日志',
    latest: '最新',
    notChecked: '未检查',
    notSelected: '未选择',
    notScanned: '未扫描',
    version: '版本',
    files: '文件',
    keepChoice: '保留',
    removeChoice: '移除',
    replaceChoice: '采用目标',
    saveTargetChoice: '另存目标',
  },
  en: {
    update: 'Update',
    backups: 'Backups',
    settings: 'Settings',
    title: 'Kairos Patch',
    subtitle: 'Minecraft modpack update workbench',
    instance: 'Current instance',
    oldPack: 'Current baseline',
    oldPackAdvanced: 'Advanced: current baseline pack (optional)',
    newPack: 'Target version',
    selectInstance: 'Select a Minecraft instance directory',
    selectOld: 'Select previous official pack folder or ZIP',
    selectNew: 'Select target official pack folder or ZIP',
    dropPath: 'Drop here',
    dropPathUnsupported: 'No dropped path was available. Use the picker button instead.',
    operationActive: 'Updating',
    operationDone: 'Update complete',
    operationFailed: 'Update failed',
    chooseFolder: 'Choose folder',
    chooseZip: 'Choose ZIP',
    compare: 'Check update',
    apply: 'Start update',
    refresh: 'Refresh backups',
    openState: 'Open .packdelta',
    safe: 'Safe to update',
    blocked: 'Needs review',
    waiting: 'Waiting for check',
    missing: 'Paths needed',
    working: 'Working',
    planTitle: 'Update detected',
    noPlan: 'Not checked',
    emptyPlanBody: 'Waiting for instance and target version.',
    safeBody: 'Update items ready.',
    conflictBody: 'Review items listed.',
    autoActions: 'Automatic',
    reviewItems: 'Needs confirmation',
    protectedItems: 'Always protected',
    conservativeMode: 'Conservative mode',
    changedFiles: 'File changes',
    protectedFiles: 'User files kept',
    conflicts: 'Needs review',
    updated: 'Updated',
    added: 'Added',
    removed: 'Removed',
    protected: 'Protected',
    viewDiff: 'View diff',
    diff: 'Diff',
    fullDiff: 'Full file diff',
    noDiff: 'Select a file to show the built-in diff.',
    diffUnavailable: 'This file cannot be previewed as text yet.',
    recentBackups: 'Backups',
    noBackups: 'No backup records yet.',
    rollback: 'Restore',
    appUpdate: 'App Update',
    feedback: 'Bug Feedback',
    createFeedback: 'Create feedback package',
    feedbackBody: 'Fill out the feedback form, create a local diagnostic package, and open the GitHub Issue template.',
    issueType: 'Issue type',
    feedbackTitle: 'Title',
    feedbackDescription: 'Description',
    reproductionSteps: 'Reproduction steps',
    includeLogs: 'Upload logs',
    includeConfig: 'Upload config',
    attachments: 'Screenshots / attachments',
    chooseAttachments: 'Choose attachments',
    clearAttachments: 'Clear attachments',
    contact: 'Contact (optional)',
    feedbackTitlePlaceholder: 'Example: update to 0.1.2 fails to launch',
    feedbackDescriptionPlaceholder: 'Describe what happened, what you expected, and what actually happened.',
    reproductionStepsPlaceholder: '1. Open the tool\n2. Select the instance and target pack\n3. Start update',
    contactPlaceholder: 'Email, GitHub ID, or another contact method',
    issueBody: 'Issue body',
    diagnosticPackage: 'Diagnostic package',
    appLogPath: 'Log file',
    updateSource: 'Update source',
    save: 'Save',
    check: 'Check',
    download: 'Download',
    install: 'Apply',
    changelog: 'Changelog',
    loadNotes: 'Load notes',
    language: 'Language',
    lastLog: 'Latest log',
    latest: 'Latest',
    notChecked: 'Not checked',
    notSelected: 'Not selected',
    notScanned: 'Not scanned',
    version: 'Version',
    files: 'Files',
    keepChoice: 'Keep',
    removeChoice: 'Remove',
    replaceChoice: 'Use target',
    saveTargetChoice: 'Save target',
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
    // Desktop settings are the source of truth when localStorage is unavailable.
  }
}

function displayName(path: string) {
  return path.split(/[\\/]/).filter(Boolean).at(-1) ?? path
}

function App() {
  const initialPreferences = useMemo(() => readCachedPreferences(), [])
  const [activePage, setActivePage] = useState<ActivePage>('update')
  const [locale, setLocale] = useState<Locale>(isLocale(initialPreferences?.locale) ? initialPreferences.locale : 'zh')
  const [instanceDir, setInstanceDir] = useState(initialPreferences?.instance_dir ?? '')
  const [oldSource, setOldSource] = useState(initialPreferences?.old_source ?? '')
  const [newSource, setNewSource] = useState(initialPreferences?.new_source ?? '')
  const [plan, setPlan] = useState<UpdatePlan | null>(null)
  const [conservativePlan, setConservativePlan] = useState<ConservativeUpdatePlan | null>(null)
  const [compareResult, setCompareResult] = useState<CompareResult | null>(null)
  const [backups, setBackups] = useState<BackupSummary[]>([])
  const [busy, setBusy] = useState('')
  const [message, setMessage] = useState('')
  const [lastLogs, setLastLogs] = useState<OperationLog[]>([])
  const [selectedDiffFile, setSelectedDiffFile] = useState<DiffFileCandidate | null>(null)
  const [diffPreview, setDiffPreview] = useState<SourceDiffPreview | null>(null)
  const [reviewChoices, setReviewChoices] = useState<Record<string, string>>({})
  const [updateSource, setUpdateSource] = useState('')
  const [appUpdate, setAppUpdate] = useState<AppUpdateCheck | null>(null)
  const [downloadedUpdate, setDownloadedUpdate] = useState<DownloadedUpdate | null>(null)
  const [changelog, setChangelog] = useState<ChangelogRelease[]>([])
  const [feedbackPackage, setFeedbackPackage] = useState<FeedbackPackage | null>(null)
  const [feedbackForm, setFeedbackForm] = useState<FeedbackFormState>(defaultFeedbackForm)
  const [operationProgress, setOperationProgress] = useState<OperationProgress | null>(null)
  const [appVersion, setAppVersion] = useState('0.1.0')
  const operationSequence = useRef(0)
  const t = copy[locale]

  const hasBaseline = Boolean(oldSource)
  const canCheck = Boolean(instanceDir && newSource && !busy)
  const baselineHasWork = Boolean(plan && (plan.added.length || plan.updated.length || plan.removed.length || plan.merged.length || plan.renamed.length))
  const conservativeHasWork = Boolean(conservativePlan && (
    conservativePlan.auto_actions.length ||
    conservativePlan.review_items.some((item) => (reviewChoices[item.id] ?? item.default_choice) !== item.default_choice)
  ))
  const canApply = Boolean(canCheck && (
    (hasBaseline && plan && !plan.conflicts.length && baselineHasWork) ||
    (!hasBaseline && conservativePlan && conservativeHasWork)
  ))
  const currentPatchVersion = conservativePlan?.target_version ?? plan?.to ?? appUpdate?.latest_version ?? appVersion

  const status = useMemo(() => {
    if (busy) return { label: t.working, tone: 'working' }
    if (!instanceDir || !newSource) return { label: t.missing, tone: 'muted' }
    if (conservativePlan?.review_items.length) return { label: t.blocked, tone: 'blocked' }
    if (plan?.conflicts.length) return { label: t.blocked, tone: 'blocked' }
    if (plan || conservativePlan) return { label: conservativePlan ? t.conservativeMode : t.safe, tone: 'safe' }
    return { label: t.waiting, tone: 'muted' }
  }, [busy, conservativePlan, instanceDir, newSource, plan, t])

  const diffFiles = useMemo<DiffFileCandidate[]>(() => {
    const candidates: DiffFileCandidate[] = []
    if (compareResult) {
      candidates.push(...compareResult.diff.updated.map((item) => ({ path: item.new.path, label: t.updated, tone: 'updated' as const })))
      candidates.push(...compareResult.diff.added.map((item) => ({ path: item.path, label: t.added, tone: 'added' as const })))
      candidates.push(...compareResult.diff.removed.map((item) => ({ path: item.path, label: t.removed, tone: 'removed' as const })))
      candidates.push(...compareResult.diff.renamed.map((item) => ({ path: item.new.path, label: `${item.old.path} -> ${item.new.path}`, tone: 'updated' as const })))
    }
    if (plan) {
      candidates.push(...plan.merged.map((item) => ({ path: item.path, label: t.updated, tone: 'updated' as const })))
      candidates.push(...plan.conflicts.map((item) => ({ path: item.path, label: item.reason, tone: 'conflict' as const })))
      candidates.push(...plan.preserved.map((path) => ({ path, label: t.protected, tone: 'protected' as const })))
    }
    if (conservativePlan) {
      candidates.push(...conservativePlan.auto_actions.map((item) => ({ path: item.path, label: item.reason, tone: 'updated' as const })))
      candidates.push(...conservativePlan.review_items.map((item) => ({ path: item.path, label: item.reason, tone: 'conflict' as const })))
      candidates.push(...conservativePlan.protected_items.map((item) => ({ path: item.path, label: item.reason, tone: 'protected' as const })))
    }
    const seen = new Set<string>()
    return candidates.filter((item) => {
      const key = `${item.tone}:${item.path}`
      if (seen.has(key)) return false
      seen.add(key)
      return true
    })
  }, [compareResult, conservativePlan, plan, t])

  const reviewCount = conservativePlan?.review_items.length ?? plan?.conflicts.length ?? 0
  const protectedCount = conservativePlan?.protected_items.length ?? plan?.preserved.length ?? 0
  const automaticCount = conservativePlan?.auto_actions.length ?? (
    plan ? plan.added.length + plan.updated.length + plan.merged.length + plan.renamed.length + plan.removed.length : 0
  )
  const versionLabel = plan ? `${plan.from} -> ${plan.to}` : (conservativePlan ? conservativePlan.target_version : t.waiting)
  const selectedDiffTitle = selectedDiffFile?.path ?? (diffFiles.length ? t.changedFiles : t.noDiff)

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
    invoke<AppPreferences>('save_app_preferences', { preferences }).then(cachePreferences).catch(() => undefined)
  }

  function changeLocale(nextLocale: Locale) {
    setLocale(nextLocale)
    persistPreferences({ locale: nextLocale })
  }

  function recordAppLog(level: OperationLog['level'], message: string, context?: string) {
    invoke('append_app_log', {
      request: {
        level,
        message,
        context: context ?? null,
      },
    }).catch(() => undefined)
  }

  useEffect(() => {
    getVersion().then(setAppVersion).catch(() => undefined)
    invoke<UpdateSourceConfig | null>('load_update_source')
      .then((config) => {
        if (config?.index_url) setUpdateSource(config.index_url)
      })
      .catch(() => undefined)
    invoke<AppPreferences>('load_app_preferences')
      .then((preferences) => {
        cachePreferences(preferences)
        if (isLocale(preferences.locale)) setLocale(preferences.locale)
        if (preferences.instance_dir) setInstanceDir(preferences.instance_dir)
        if (preferences.old_source) setOldSource(preferences.old_source)
        if (preferences.new_source) setNewSource(preferences.new_source)
      })
      .catch(() => undefined)
  }, [])

  useEffect(() => {
    let mounted = true
    let dispose: (() => void) | undefined
    listen<OperationProgress>('operation-progress', (event) => {
      if (mounted) setOperationProgress(event.payload)
    })
      .then((unlisten) => {
        dispose = unlisten
      })
      .catch(() => undefined)
    return () => {
      mounted = false
      dispose?.()
    }
  }, [])

  async function pickDirectory(key: PreferencePathKey, setter: (path: string) => void) {
    const selected = await open({ directory: true, multiple: false })
    if (typeof selected === 'string') {
      setter(selected)
      persistPreferences({ [key]: selected })
    }
  }

  async function pickZipSource(key: Extract<PreferencePathKey, 'old_source' | 'new_source'>, setter: (path: string) => void) {
    const selected = await open({
      directory: false,
      multiple: false,
      filters: [{ name: 'ZIP', extensions: ['zip'] }],
    })
    if (typeof selected === 'string') {
      setter(selected)
      persistPreferences({ [key]: selected })
    }
  }

  function updateFeedbackForm(patch: Partial<FeedbackFormState>) {
    setFeedbackForm((current) => ({ ...current, ...patch }))
  }

  async function pickFeedbackAttachments() {
    const selected = await open({
      directory: false,
      multiple: true,
      filters: [
        { name: 'Attachments', extensions: ['png', 'jpg', 'jpeg', 'webp', 'gif', 'txt', 'log', 'json', 'zip'] },
      ],
    })
    if (Array.isArray(selected)) {
      updateFeedbackForm({ attachmentPaths: selected })
    } else if (typeof selected === 'string') {
      updateFeedbackForm({ attachmentPaths: [selected] })
    }
  }

  function acceptDroppedPath(key: PreferencePathKey, setter: (path: string) => void, path: string | null) {
    if (!path) {
      setMessage(t.dropPathUnsupported)
      return
    }
    setter(path)
    persistPreferences({ [key]: path })
  }

  async function runPreview() {
    if (!canCheck) return
    setBusy('preview')
    setMessage(locale === 'zh' ? '正在检查更新计划...' : 'Checking update plan...')
    try {
      if (oldSource) {
        const [compare, nextPlan] = await Promise.all([
          invoke<CompareResult>('compare_pack_sources', { oldSource, newSource }),
          invoke<UpdatePlan>('preview_update', { instanceDir, oldSource, newSource }),
        ])
        setCompareResult(compare)
        setPlan(nextPlan)
        setConservativePlan(null)
        setMessage(nextPlan.conflicts.length ? t.conflictBody : t.safeBody)
        recordAppLog(nextPlan.conflicts.length ? 'warn' : 'info', 'Baseline update preview completed', `${nextPlan.conflicts.length} conflicts`)
      } else {
        const nextPlan = await invoke<ConservativeUpdatePlan>('preview_conservative_update', {
          instanceDir,
          targetSource: newSource,
        })
        setConservativePlan(nextPlan)
        setReviewChoices(Object.fromEntries(nextPlan.review_items.map((item) => [item.id, item.default_choice])))
        setPlan(null)
        setCompareResult(null)
        setMessage(nextPlan.review_items.length ? t.conflictBody : t.safeBody)
        recordAppLog(nextPlan.review_items.length ? 'warn' : 'info', 'Conservative update preview completed', `${nextPlan.review_items.length} review items`)
      }
      setSelectedDiffFile(null)
      setDiffPreview(null)
      await refreshBackups(true)
    } catch (error) {
      setMessage(String(error))
      recordAppLog('error', 'Update preview failed', String(error))
    } finally {
      setBusy('')
    }
  }

  async function runApply() {
    if (!canApply) return
    operationSequence.current += 1
    const operationId = `apply-${operationSequence.current}`
    const activeMessage = locale === 'zh' ? '正在备份并应用更新...' : 'Backing up and applying update...'
    setBusy('apply')
    setMessage(activeMessage)
    setOperationProgress({
      operation_id: operationId,
      stage: 'starting',
      message: activeMessage,
      current: 0,
      total: 1,
      percent: 0,
      done: false,
    })
    try {
      if (hasBaseline) {
        const result = await invoke<ApplyResult>('apply_update_tracked', { operationId, instanceDir, oldSource, newSource })
        setLastLogs(result.logs)
        setPlan(result.plan)
        setMessage(result.backup_id ? `${locale === 'zh' ? '更新完成，备份 ID' : 'Update complete. Backup ID'}: ${result.backup_id}` : locale === 'zh' ? '无需写入，已经是目标版本。' : 'No writes needed. Already at target version.')
        recordAppLog('info', 'Baseline update apply completed', result.backup_id ?? 'no backup')
      } else {
        const result = await invoke<ConservativeApplyResult>('apply_conservative_update_tracked', {
          operationId,
          instanceDir,
          targetSource: newSource,
          reviewChoices,
        })
        setLastLogs(result.logs.map((log) => ({ level: 'info' as const, message: log })))
        const nextPlan = await invoke<ConservativeUpdatePlan>('preview_conservative_update', {
          instanceDir,
          targetSource: newSource,
        })
        setConservativePlan(nextPlan)
        setReviewChoices(Object.fromEntries(nextPlan.review_items.map((item) => [item.id, item.default_choice])))
        setMessage(result.backup_id ? `${locale === 'zh' ? '更新完成，备份 ID' : 'Update complete. Backup ID'}: ${result.backup_id}` : locale === 'zh' ? '无需写入。' : 'No writes needed.')
        recordAppLog('info', 'Conservative update apply completed', result.backup_id ?? 'no backup')
      }
      await refreshBackups(true)
    } catch (error) {
      const errorMessage = String(error)
      setMessage(errorMessage)
      setOperationProgress((current) => ({
        operation_id: current?.operation_id ?? operationId,
        stage: 'failed',
        message: errorMessage,
        current: current?.current ?? 0,
        total: current?.total ?? 1,
        percent: current?.percent ?? 0,
        path: current?.path,
        done: true,
      }))
      recordAppLog('error', 'Update apply failed', errorMessage)
    } finally {
      setBusy('')
    }
  }

  async function refreshBackups(silent = false) {
    if (!instanceDir) return
    if (!silent) setBusy('backups')
    try {
      const result = await invoke<BackupSummary[]>('list_backups', { instanceDir })
      setBackups(result)
      if (!silent) setMessage(result.length ? `${result.length} ${t.backups}` : t.noBackups)
    } catch (error) {
      if (!silent) setMessage(String(error))
    } finally {
      if (!silent) setBusy('')
    }
  }

  async function rollback(backupId: string) {
    if (!instanceDir) return
    setBusy(`rollback:${backupId}`)
    try {
      const result = await invoke<{ restored_files: number }>('rollback', { instanceDir, backupId })
      setMessage(locale === 'zh' ? `已恢复 ${result.restored_files} 个文件。` : `Restored ${result.restored_files} files.`)
      recordAppLog('info', 'Rollback completed', `${backupId}: ${result.restored_files} files`)
      await refreshBackups(true)
    } catch (error) {
      setMessage(String(error))
      recordAppLog('error', 'Rollback failed', String(error))
    } finally {
      setBusy('')
    }
  }

  async function openPackDelta() {
    if (!instanceDir) return
    await invoke('open_folder', { path: `${instanceDir}\\.packdelta` }).catch((error) => setMessage(String(error)))
  }

  async function openDiffFile(file: DiffFileCandidate) {
    setSelectedDiffFile(file)
    setDiffPreview(null)
    if (!newSource) return
    try {
      const result = oldSource
        ? await invoke<SourceDiffPreview>('read_source_diff', {
          oldSource,
          newSource,
          path: file.path,
        })
        : await invoke<SourceDiffPreview>('read_source_diff', {
          oldSource: instanceDir,
          newSource,
          path: file.path,
        })
      setDiffPreview(result)
    } catch (error) {
      setDiffPreview({
        path: file.path,
        old_text: '',
        new_text: String(error),
        language: 'plaintext',
        notice: t.diffUnavailable,
      })
    }
  }

  async function saveUpdateSource() {
    if (!updateSource) return
    setBusy('save-update-source')
    try {
      const result = await invoke<UpdateSourceConfig>('save_update_source', { indexUrl: updateSource })
      setUpdateSource(result.index_url)
      setMessage(locale === 'zh' ? '更新源已保存。' : 'Update source saved.')
      recordAppLog('info', 'Update source saved', result.index_url)
    } catch (error) {
      setMessage(String(error))
      recordAppLog('error', 'Saving update source failed', String(error))
    } finally {
      setBusy('')
    }
  }

  async function checkAppUpdate() {
    if (!updateSource) return
    setBusy('check-app-update')
    try {
      const result = await invoke<AppUpdateCheck>('check_app_update', { indexUrl: updateSource })
      setAppUpdate(result)
      setMessage(result.update_available ? `${t.latest}: ${result.latest_version}` : `${t.latest}: ${result.current_version}`)
      recordAppLog('info', 'App update check completed', `${result.current_version} -> ${result.latest_version}`)
    } catch (error) {
      setMessage(String(error))
      recordAppLog('error', 'App update check failed', String(error))
    } finally {
      setBusy('')
    }
  }

  async function downloadAppUpdate() {
    if (!appUpdate?.release) return
    setBusy('download-app-update')
    try {
      const result = await invoke<DownloadedUpdate>('download_app_update', { release: appUpdate.release })
      setDownloadedUpdate(result)
      setMessage(result.archive_path)
      recordAppLog('info', 'App update downloaded', result.archive_path)
    } catch (error) {
      setMessage(String(error))
      recordAppLog('error', 'App update download failed', String(error))
    } finally {
      setBusy('')
    }
  }

  async function installPortableUpdate() {
    if (!downloadedUpdate) return
    setBusy('install-app-update')
    try {
      await invoke('install_portable_update', { downloaded: downloadedUpdate })
    } catch (error) {
      setMessage(String(error))
      recordAppLog('error', 'Portable update install failed', String(error))
      setBusy('')
    }
  }

  async function fetchChangelog() {
    if (!updateSource) return
    setBusy('changelog')
    try {
      const result = await invoke<ChangelogRelease[]>('fetch_changelog', { indexUrl: updateSource })
      setChangelog(result)
      recordAppLog('info', 'Changelog loaded', `${result.length} releases`)
    } catch (error) {
      setMessage(String(error))
      recordAppLog('error', 'Changelog load failed', String(error))
    } finally {
      setBusy('')
    }
  }

  async function createFeedbackPackage() {
    const title = feedbackForm.title.trim()
    const description = feedbackForm.description.trim()
    if (!title || !description) {
      setMessage(locale === 'zh' ? '请填写反馈标题和描述。' : 'Title and description are required.')
      return
    }
    setBusy('feedback')
    try {
      const result = await invoke<FeedbackPackage>('create_feedback_package', {
        request: {
          issue_type: feedbackForm.issueType,
          title,
          description,
          reproduction_steps: feedbackForm.reproductionSteps.trim(),
          include_logs: feedbackForm.includeLogs,
          include_config: feedbackForm.includeConfig,
          attachment_paths: feedbackForm.attachmentPaths,
          contact: feedbackForm.contact.trim() || null,
          instance_dir: instanceDir || null,
          old_source: oldSource || null,
          new_source: newSource || null,
          update_source: updateSource || null,
          patch_version: currentPatchVersion || null,
          ui_logs: lastLogs,
          open_issue: true,
        },
      })
      setFeedbackPackage(result)
      setMessage(result.archive_path)
      recordAppLog('info', 'Feedback package created', result.archive_path)
    } catch (error) {
      setMessage(String(error))
      recordAppLog('error', 'Feedback package creation failed', String(error))
    } finally {
      setBusy('')
    }
  }

  return (
    <main className="app-shell" data-locale={locale}>
      <aside className="rail">
        <div className="brand-mark">
          <img src="/brand/kairos-patch-mark.svg" alt="Kairos Patch" />
        </div>
        <nav className="rail-nav" aria-label="Primary">
          <button type="button" className={activePage === 'update' ? 'active' : ''} onClick={() => setActivePage('update')}>
            <GitCompare size={17} />
            <span>{t.update}</span>
          </button>
          <button type="button" className={activePage === 'backups' ? 'active' : ''} onClick={() => setActivePage('backups')}>
            <ArchiveRestore size={17} />
            <span>{t.backups}</span>
          </button>
          <button type="button" className={activePage === 'settings' ? 'active' : ''} onClick={() => setActivePage('settings')}>
            <Settings size={17} />
            <span>{t.settings}</span>
          </button>
        </nav>
      </aside>

      <section className="workspace">
        <header className="topbar">
          <div>
            <strong>{t.title}</strong>
            <span>v{appVersion}</span>
          </div>
          <p>{t.subtitle}</p>
        </header>
        {operationProgress && (
          <OperationToast
            progress={operationProgress}
            activeLabel={t.operationActive}
            doneLabel={t.operationDone}
            failedLabel={t.operationFailed}
            onDismiss={() => setOperationProgress(null)}
          />
        )}

        {activePage === 'update' && (
          <section className="update-workbench utility-workbench">
            <header className="utility-toolbar">
              <div className="utility-title">
                <strong>{displayName(instanceDir) || t.notSelected}</strong>
                <span>{versionLabel}</span>
              </div>
              <span className={`task-status ${status.tone}`}>{status.label}</span>
              <div className="utility-actions">
                <button type="button" className="plain-button" onClick={runPreview} disabled={!canCheck}>
                  <GitCompare size={15} />
                  {t.compare}
                </button>
                <button type="button" className="plain-button strong" onClick={runApply} disabled={!canApply}>
                  <Rocket size={15} />
                  {t.apply}
                </button>
                <button type="button" className="icon-button" onClick={openPackDelta} disabled={!instanceDir} title={t.openState}>
                  <FolderOpen size={15} />
                </button>
              </div>
            </header>

            <section className="utility-sources">
              <SourcePicker
                icon={<HardDrive size={15} />}
                label={t.instance}
                value={instanceDir}
                fallback={t.selectInstance}
                onPick={() => pickDirectory('instance_dir', setInstanceDir)}
                dropHint={t.dropPath}
                onDropPath={(path) => acceptDroppedPath('instance_dir', setInstanceDir, path)}
              />
              <SourcePicker
                icon={<Folder size={15} />}
                label={t.newPack}
                value={newSource}
                fallback={t.selectNew}
                onPick={() => pickDirectory('new_source', setNewSource)}
                onPickZip={() => pickZipSource('new_source', setNewSource)}
                dropHint={t.dropPath}
                onDropPath={(path) => acceptDroppedPath('new_source', setNewSource, path)}
              />
              <details className="baseline-field">
                <summary>{t.oldPackAdvanced}</summary>
                <SourcePicker
                  icon={<FolderOpen size={15} />}
                  label={t.oldPack}
                  value={oldSource}
                  fallback={t.selectOld}
                  onPick={() => pickDirectory('old_source', setOldSource)}
                  onPickZip={() => pickZipSource('old_source', setOldSource)}
                  dropHint={t.dropPath}
                  onDropPath={(path) => acceptDroppedPath('old_source', setOldSource, path)}
                />
              </details>
            </section>

            <section className="tool-split">
              <aside className="changes-pane">
                <div className="pane-head">
                  <h2>{t.changedFiles}</h2>
                  <span>{diffFiles.length}</span>
                </div>
                <div className="change-list">
                  {diffFiles.length ? diffFiles.slice(0, 140).map((file) => (
                    <button
                      type="button"
                      className={`change-row ${file.tone} ${selectedDiffFile?.path === file.path ? 'active' : ''}`}
                      key={`${file.tone}:${file.path}`}
                      onClick={() => openDiffFile(file)}
                    >
                      <span className="change-kind">{file.tone === 'added' ? '+' : file.tone === 'removed' ? '-' : file.tone === 'conflict' ? '!' : file.tone === 'protected' ? '=' : '~'}</span>
                      <span className="change-path">{file.path}</span>
                      <small>{file.label}</small>
                    </button>
                  )) : (
                    <div className="pane-empty">
                      <span>{t.noPlan}</span>
                      <small>{message || t.emptyPlanBody}</small>
                    </div>
                  )}
                </div>

                {conservativePlan?.review_items.length ? (
                  <div className="review-inline">
                    <div className="pane-head compact">
                      <h2>{t.reviewItems}</h2>
                      <span>{conservativePlan.review_items.length}</span>
                    </div>
                    {conservativePlan.review_items.map((item) => (
                      <div className="review-row" key={item.id}>
                        <button type="button" onClick={() => openDiffFile({ path: item.path, label: item.reason, tone: 'conflict' })}>
                          <span>{item.mod_name ?? displayName(item.path)}</span>
                          <small>{item.path}</small>
                        </button>
                        <select
                          value={reviewChoices[item.id] ?? item.default_choice}
                          onChange={(event) => setReviewChoices((current) => ({ ...current, [item.id]: event.target.value }))}
                        >
                          {item.choices.map((choice) => (
                            <option key={choice} value={choice}>
                              {{
                                keep: t.keepChoice,
                                remove: t.removeChoice,
                                replace_with_target: t.replaceChoice,
                                save_target_as_new: t.saveTargetChoice,
                                use_target: t.replaceChoice,
                              }[choice] ?? choice}
                            </option>
                          ))}
                        </select>
                      </div>
                    ))}
                  </div>
                ) : null}
              </aside>

              <section className="diff-pane">
                <div className="pane-head">
                  <h2>{selectedDiffTitle}</h2>
                  <span>{selectedDiffFile?.label ?? status.label}</span>
                </div>
                {diffPreview ? (
                  <>
                    {diffPreview.notice && <p className="diff-notice">{diffPreview.notice}</p>}
                    <DiffEditor
                      height="100%"
                      language={diffPreview.language}
                      original={diffPreview.old_text}
                      modified={diffPreview.new_text}
                      theme="vs"
                      options={{
                        readOnly: true,
                        renderSideBySide: true,
                        minimap: { enabled: false },
                        fontSize: 12,
                        lineNumbersMinChars: 3,
                        scrollBeyondLastLine: false,
                        wordWrap: 'on',
                      }}
                    />
                  </>
                ) : (
                  <div className="diff-empty">
                    <GitCompare size={20} />
                    <p>{selectedDiffFile ? t.diffUnavailable : t.noDiff}</p>
                  </div>
                )}
              </section>
            </section>

            <footer className="utility-statusbar">
              <span>{message || ((plan || conservativePlan) ? ((reviewCount > 0) ? t.conflictBody : t.safeBody) : t.emptyPlanBody)}</span>
              <span>{automaticCount} {t.autoActions}</span>
              <span>{reviewCount} {t.reviewItems}</span>
              <span>{protectedCount} {t.protectedItems}</span>
            </footer>
          </section>
        )}

        {activePage === 'backups' && (
          <section className="simple-page">
            <div className="page-head">
              <h1>{t.recentBackups}</h1>
              <button type="button" className="plain-button" onClick={() => refreshBackups()} disabled={!instanceDir || Boolean(busy)}>
                <RefreshCw size={16} />
                {t.refresh}
              </button>
            </div>
            <SourcePicker
              icon={<HardDrive size={17} />}
              label={t.instance}
              value={instanceDir}
              fallback={t.selectInstance}
              onPick={() => pickDirectory('instance_dir', setInstanceDir)}
              dropHint={t.dropPath}
              onDropPath={(path) => acceptDroppedPath('instance_dir', setInstanceDir, path)}
            />
            <div className="backup-list">
              {backups.length ? backups.map((backup) => (
                <article className="backup-row" key={backup.id}>
                  <div>
                    <strong>{backup.from} {'->'} {backup.to}</strong>
                    <span>{backup.id}</span>
                  </div>
                  <small>{backup.file_count} {t.files}</small>
                  <button type="button" className="plain-button" onClick={() => rollback(backup.id)} disabled={Boolean(busy)}>
                    {t.rollback}
                  </button>
                </article>
              )) : <p className="quiet">{t.noBackups}</p>}
            </div>
          </section>
        )}

        {activePage === 'settings' && (
          <section className="simple-page settings-page">
            <div className="page-head">
              <h1>{t.settings}</h1>
            </div>
            <section className="settings-section">
              <h2>{t.language}</h2>
              <div className="segmented">
                <button type="button" className={locale === 'zh' ? 'active' : ''} onClick={() => changeLocale('zh')}>中文</button>
                <button type="button" className={locale === 'en' ? 'active' : ''} onClick={() => changeLocale('en')}>EN</button>
              </div>
            </section>
            <section className="settings-section">
              <h2>{t.appUpdate}</h2>
              <input
                className="text-input"
                value={updateSource}
                onChange={(event) => setUpdateSource(event.target.value)}
                placeholder="https://github.com/SevenThRe/karios-patch/releases/latest/download/release-index.json"
              />
              <div className="button-row">
                <button type="button" className="plain-button" onClick={saveUpdateSource} disabled={!updateSource || Boolean(busy)}>{t.save}</button>
                <button type="button" className="plain-button" onClick={checkAppUpdate} disabled={!updateSource || Boolean(busy)}>{t.check}</button>
                <button type="button" className="plain-button" onClick={downloadAppUpdate} disabled={!appUpdate?.release || Boolean(busy)}><Download size={15} />{t.download}</button>
                <button type="button" className="plain-button" onClick={installPortableUpdate} disabled={!downloadedUpdate || Boolean(busy)}>{t.install}</button>
              </div>
              <p className="quiet">{appUpdate ? `${appUpdate.current_version} -> ${appUpdate.latest_version}` : t.notChecked}</p>
            </section>
            <section className="settings-section">
              <h2>{t.changelog}</h2>
              <button type="button" className="plain-button" onClick={fetchChangelog} disabled={!updateSource || Boolean(busy)}>
                <History size={15} />
                {t.loadNotes}
              </button>
              <div className="changelog-list">
                {changelog.slice(0, 5).map((release) => (
                  <article key={release.url}>
                    <strong>{release.title}</strong>
                    <span>{release.version}</span>
                  </article>
                ))}
              </div>
            </section>
            <section className="settings-section">
              <h2>{t.feedback}</h2>
              <p className="quiet">{t.feedbackBody}</p>
              <div className="feedback-form">
                <label>
                  <span>{t.issueType}</span>
                  <select
                    className="text-input"
                    value={feedbackForm.issueType}
                    onChange={(event) => updateFeedbackForm({ issueType: event.target.value as FeedbackIssueType })}
                  >
                    {feedbackIssueTypes.map((issueType) => (
                      <option key={issueType} value={issueType}>{issueType}</option>
                    ))}
                  </select>
                </label>
                <label>
                  <span>{t.feedbackTitle}</span>
                  <input
                    className="text-input"
                    value={feedbackForm.title}
                    onChange={(event) => updateFeedbackForm({ title: event.target.value })}
                    placeholder={t.feedbackTitlePlaceholder}
                  />
                </label>
                <label className="wide">
                  <span>{t.feedbackDescription}</span>
                  <textarea
                    className="text-area"
                    value={feedbackForm.description}
                    onChange={(event) => updateFeedbackForm({ description: event.target.value })}
                    placeholder={t.feedbackDescriptionPlaceholder}
                    rows={4}
                  />
                </label>
                <label className="wide">
                  <span>{t.reproductionSteps}</span>
                  <textarea
                    className="text-area"
                    value={feedbackForm.reproductionSteps}
                    onChange={(event) => updateFeedbackForm({ reproductionSteps: event.target.value })}
                    placeholder={t.reproductionStepsPlaceholder}
                    rows={4}
                  />
                </label>
                <div className="feedback-options">
                  <label>
                    <input
                      type="checkbox"
                      checked={feedbackForm.includeLogs}
                      onChange={(event) => updateFeedbackForm({ includeLogs: event.target.checked })}
                    />
                    <span>{t.includeLogs}</span>
                  </label>
                  <label>
                    <input
                      type="checkbox"
                      checked={feedbackForm.includeConfig}
                      onChange={(event) => updateFeedbackForm({ includeConfig: event.target.checked })}
                    />
                    <span>{t.includeConfig}</span>
                  </label>
                </div>
                <div className="feedback-attachments wide">
                  <div>
                    <span>{t.attachments}</span>
                    <small>{feedbackForm.attachmentPaths.length ? feedbackForm.attachmentPaths.map(displayName).join(', ') : t.notSelected}</small>
                  </div>
                  <div className="button-row">
                    <button type="button" className="plain-button" onClick={pickFeedbackAttachments} disabled={Boolean(busy)}>
                      <Paperclip size={15} />
                      {t.chooseAttachments}
                    </button>
                    <button type="button" className="plain-button" onClick={() => updateFeedbackForm({ attachmentPaths: [] })} disabled={!feedbackForm.attachmentPaths.length || Boolean(busy)}>
                      {t.clearAttachments}
                    </button>
                  </div>
                </div>
                <label className="wide">
                  <span>{t.contact}</span>
                  <input
                    className="text-input"
                    value={feedbackForm.contact}
                    onChange={(event) => updateFeedbackForm({ contact: event.target.value })}
                    placeholder={t.contactPlaceholder}
                  />
                </label>
              </div>
              <div className="button-row">
                <button type="button" className="plain-button" onClick={createFeedbackPackage} disabled={Boolean(busy)}>
                  <Bug size={15} />
                  {t.createFeedback}
                </button>
                {feedbackPackage && (
                  <button type="button" className="plain-button" onClick={() => invoke('open_folder', { path: feedbackPackage.archive_path }).catch((error) => setMessage(String(error)))}>
                    <ExternalLink size={15} />
                    {t.diagnosticPackage}
                  </button>
                )}
              </div>
              {feedbackPackage && (
                <div className="feedback-paths">
                  <span>{t.diagnosticPackage}: {feedbackPackage.archive_path}</span>
                  <span>{t.issueBody}: {feedbackPackage.issue_body_path}</span>
                  <span>{t.appLogPath}: {feedbackPackage.app_log_path}</span>
                </div>
              )}
            </section>
            {lastLogs.length > 0 && (
              <section className="settings-section">
                <h2>{t.lastLog}</h2>
                <LogList logs={lastLogs} />
              </section>
            )}
          </section>
        )}
      </section>
    </main>
  )
}

function OperationToast({
  progress,
  activeLabel,
  doneLabel,
  failedLabel,
  onDismiss,
}: {
  progress: OperationProgress
  activeLabel: string
  doneLabel: string
  failedLabel: string
  onDismiss: () => void
}) {
  const isFailed = progress.stage === 'failed'
  const label = isFailed ? failedLabel : progress.done ? doneLabel : activeLabel
  const hasMeasuredProgress = progress.total > 1

  return (
    <section className={`operation-toast ${progress.done ? 'done' : 'active'} ${isFailed ? 'failed' : ''}`}>
      <div className="operation-toast-head">
        <div>
          <strong>{label}</strong>
          <span>{progress.stage}</span>
        </div>
        <button type="button" onClick={onDismiss} aria-label="Dismiss"><X size={14} /></button>
      </div>
      <p>{progress.message}</p>
      {progress.path && <small title={progress.path}>{progress.path}</small>}
      <div className={`operation-progress ${hasMeasuredProgress ? '' : 'indeterminate'}`}>
        <span style={{ width: `${hasMeasuredProgress ? progress.percent : 45}%` }} />
      </div>
      <footer>
        <span>{hasMeasuredProgress ? `${progress.percent}%` : ''}</span>
        {hasMeasuredProgress && <span>{progress.current}/{progress.total}</span>}
      </footer>
    </section>
  )
}

function SourcePicker({
  icon,
  label,
  value,
  fallback,
  onPick,
  onPickZip,
  dropHint,
  onDropPath,
}: {
  icon: React.ReactNode
  label: string
  value: string
  fallback: string
  onPick: () => void
  onPickZip?: () => void
  dropHint?: string
  onDropPath?: (path: string | null) => void
}) {
  const [isDragging, setIsDragging] = useState(false)

  function readDroppedPath(event: React.DragEvent<HTMLElement>) {
    const file = event.dataTransfer.files.item(0) as (File & { path?: string }) | null
    return file?.path || file?.webkitRelativePath || null
  }

  return (
    <article
      className={`source-picker ${isDragging ? 'dragging' : ''}`}
      onDragEnter={(event) => {
        if (!onDropPath) return
        event.preventDefault()
        setIsDragging(true)
      }}
      onDragOver={(event) => {
        if (!onDropPath) return
        event.preventDefault()
        event.dataTransfer.dropEffect = 'copy'
      }}
      onDragLeave={(event) => {
        if (!event.currentTarget.contains(event.relatedTarget as Node | null)) {
          setIsDragging(false)
        }
      }}
      onDrop={(event) => {
        if (!onDropPath) return
        event.preventDefault()
        setIsDragging(false)
        onDropPath(readDroppedPath(event))
      }}
    >
      <div className="source-label">
        {icon}
        <span>{label}</span>
      </div>
      <div className="source-value" title={value || fallback}>{value || fallback}</div>
      {dropHint && <div className="source-drop-hint">{dropHint}</div>}
      <div className="source-buttons">
        <button type="button" onClick={onPick} aria-label={label}><Folder size={16} /></button>
        <button type="button" onClick={onPickZip} disabled={!onPickZip} aria-label="ZIP"><MoreHorizontal size={16} /></button>
      </div>
    </article>
  )
}

function LogList({ logs }: { logs: OperationLog[] }) {
  return (
    <ol className="log-list">
      {logs.slice(-8).map((log, index) => (
        <li className={log.level} key={`${index}:${log.message}`}>
          <span>{log.level}</span>
          <p>{log.message}</p>
        </li>
      ))}
    </ol>
  )
}

export default App
