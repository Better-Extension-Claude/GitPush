// Ce qui est gardé par projet (dans $.store, clé `projet:<dossier>`).
export type ProjectConfig = {
  remote: string
  branch: string
  selection: string[]
  lastBy: 'vous' | 'Claude' | null
  lastAt: string | null
}

export type View = {
  isOpen: boolean
  tab: 'fichiers' | 'depot'
  expanded: string[]
  status: string
  statusKind: 'info' | 'ok' | 'erreur'
  isBusy: boolean
  canForce: boolean
  message: string
  repoName: string
  isPublic: boolean
  repos: string[]
  branches: string[]
}

declare module 'claude-code' {
  interface PluginState {
    'push-projet': {
      cfg: Shaped<ProjectConfig | null>
      sel: Shaped<string[]>
      view: Shaped<View>
    }
  }
}
