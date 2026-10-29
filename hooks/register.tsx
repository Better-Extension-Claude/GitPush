import { atom, read, update } from 'claude-code'
import type { EngineInterface, Register } from 'claude-code'

import type { ProjectConfig, View } from '../types'

const HIDDEN = new Set(['.git'])
const MAX_ROWS = 400

const cfgAtom = atom({ plugin: 'push-projet', key: 'cfg' } as const, null, {
  shape: 'cfg-1',
})
const selAtom = atom({ plugin: 'push-projet', key: 'sel' } as const, [], {
  shape: 'sel-1',
})
const INITIAL_VIEW: View = {
  isOpen: false,
  tab: 'fichiers',
  expanded: [],
  status: '',
  statusKind: 'info',
  isBusy: false,
  canForce: false,
  message: '',
  repoName: '',
  isPublic: false,
  repos: [],
  branches: [],
}
const viewAtom = atom({ plugin: 'push-projet', key: 'view' } as const, INITIAL_VIEW, {
  shape: 'view-2',
})

type $ = EngineInterface
type Row = { path: string; name: string; isDir: boolean; depth: number }

const storeKey = (cwd: string) => `projet:${cwd}`
const parentOf = (p: string) => (p.includes('/') ? p.slice(0, p.lastIndexOf('/')) : '')
const isUnder = (p: string, dir: string) => p.startsWith(dir + '/')
const kindOf = (status: string): View['statusKind'] =>
  /^(Échec|Erreur|Aucun|gh indisponible|Donne)/.test(status)
    ? 'erreur'
    : /^(Poussé|Déjà|Dépôt créé)/.test(status)
      ? 'ok'
      : 'info'
const setView = ($: $, patch: Partial<View>) =>
  update($, viewAtom, v => ({
    ...v,
    ...patch,
    ...(patch.status !== undefined && { statusKind: kindOf(patch.status) }),
  }))

async function git($: $, args: string[], timeoutMs = 60_000) {
  return $.process.run(['git', ...args], { cwd: await $.session.cwd(), timeoutMs })
}

async function loadConfig($: $) {
  const cwd = await $.session.cwd()
  const saved = (await $.store.get(storeKey(cwd))) as ProjectConfig | undefined
  let cfg = saved ?? null
  if (!cfg) {
    // Pas encore de config : on reprend ce que le dépôt git local sait déjà.
    const remote = await git($, ['remote', 'get-url', 'origin']).catch(() => null)
    const branch = await git($, ['branch', '--show-current']).catch(() => null)
    if (remote?.exitCode === 0) {
      cfg = {
        remote: remote.stdout.trim(),
        branch: branch?.stdout.trim() || 'main',
        selection: [],
        lastBy: null,
        lastAt: null,
      }
    }
  }
  await update($, cfgAtom, () => cfg)
  await update($, selAtom, () => cfg?.selection ?? [])
}

async function saveConfig($: $, patch: Partial<ProjectConfig>) {
  const cwd = await $.session.cwd()
  const prev = (await read($, cfgAtom)) ?? {
    remote: '',
    branch: 'main',
    selection: [],
    lastBy: null,
    lastAt: null,
  }
  const next: ProjectConfig = { ...prev, ...patch }
  await $.store.set(storeKey(cwd), next)
  await update($, cfgAtom, () => next)

  return next
}

async function listDir($: $, dir: string) {
  const entries = await $.fs.list(dir || '.').catch(() => [])

  return entries
    .filter(one => !HIDDEN.has(one.name) && one.kind !== 'other')
    .sort((a, b) => (a.kind === b.kind ? a.name.localeCompare(b.name) : a.kind === 'dir' ? -1 : 1))
    .map(one => ({ path: dir ? `${dir}/${one.name}` : one.name, name: one.name, isDir: one.kind === 'dir' }))
}

async function buildRows($: $, expanded: string[]) {
  const rows: Row[] = []
  const walk = async (dir: string, depth: number) => {
    for (const one of await listDir($, dir)) {
      if (rows.length >= MAX_ROWS) {
        return
      }
      rows.push({ ...one, depth })
      if (one.isDir && expanded.includes(one.path)) {
        await walk(one.path, depth + 1)
      }
    }
  }
  await walk('', 0)

  return rows
}

function checkState(sel: string[], path: string): 'oui' | 'partiel' | 'non' {
  if (sel.some(s => s === path || isUnder(path, s))) {
    return 'oui'
  }

  return sel.some(s => isUnder(s, path)) ? 'partiel' : 'non'
}

async function toggle($: $, path: string) {
  const sel = await read($, selAtom)
  if (checkState(sel, path) !== 'oui') {
    await update($, selAtom, list => [...list.filter(s => !isUnder(s, path)), path])

    return
  }
  const ancestor = sel.find(s => s === path || isUnder(path, s))!
  let next = sel.filter(s => s !== ancestor)
  // Décocher un élément d'un dossier coché : on garde ses frères et sœurs.
  let dir = path
  while (dir !== ancestor) {
    const parent = parentOf(dir)
    const siblings = await listDir($, parent)
    next = [...next, ...siblings.map(s => s.path).filter(p => p !== dir)]
    dir = parent
  }
  await update($, selAtom, () => next)
}

async function existing($: $, sel: string[]) {
  const kept: string[] = []
  for (const p of sel) {
    if (await $.fs.exists(p)) {
      kept.push(p)
    }
  }

  return kept
}

async function push($: $, isForced = false): Promise<string> {
  const cfg = await read($, cfgAtom)
  const sel = await existing($, await read($, selAtom))
  const view = await read($, viewAtom)
  if (!cfg?.remote) {
    return 'Aucun dépôt configuré : ouvre « Configurer ».'
  }
  if (sel.length === 0) {
    return 'Aucun fichier coché.'
  }
  const branch = cfg.branch || 'main'
  const fail = (step: string, r: { stderr: string; stdout: string }) =>
    `Échec (${step}) : ${(r.stderr || r.stdout).trim().split('\n').slice(-3).join(' ')}`

  if (!(await $.fs.exists('.git'))) {
    const r = await git($, ['init', '-q', '-b', branch])
    if (r.exitCode !== 0) {
      return fail('git init', r)
    }
  }
  const current = await git($, ['remote', 'get-url', 'origin'])
  if (current.exitCode !== 0) {
    await git($, ['remote', 'add', 'origin', cfg.remote])
  } else if (current.stdout.trim() !== cfg.remote) {
    await git($, ['remote', 'set-url', 'origin', cfg.remote])
  }

  // On se place sur la branche choisie sans toucher aux fichiers du disque,
  // puis on part de l'état distant : le push est toujours une simple avance.
  await git($, ['symbolic-ref', 'HEAD', `refs/heads/${branch}`])
  const fetched = await git($, ['fetch', '-q', 'origin', branch], 180_000)
  const hasRemote = fetched.exitCode === 0
  if (hasRemote) {
    await git($, ['reset', '-q', `origin/${branch}`])
  }
  await git($, ['rm', '-r', '-q', '--cached', '--ignore-unmatch', '--', '.'])
  const added = await git($, ['add', '-A', '--', ...sel], 120_000)
  if (added.exitCode !== 0) {
    return fail('git add', added)
  }
  const hasChanges = (await git($, ['diff', '--cached', '--quiet'])).exitCode !== 0
  const hasHead = (await git($, ['rev-parse', '-q', '--verify', 'HEAD'])).exitCode === 0
  if (!hasChanges && hasRemote) {
    await saveConfig($, { selection: sel, lastBy: 'vous', lastAt: new Date().toISOString() })

    return `Déjà à jour sur ${branch}.`
  }
  if (hasChanges || !hasHead) {
    const message = view.message.trim() || `Mise à jour du ${new Date().toLocaleString('fr-FR')}`
    const committed = await git($, ['commit', '-q', '--allow-empty', '-m', message])
    if (committed.exitCode !== 0) {
      return fail('git commit', committed)
    }
  }
  const pushed = await git(
    $,
    ['push', '-u', ...(isForced ? ['--force'] : []), 'origin', `HEAD:refs/heads/${branch}`],
    300_000,
  )
  if (pushed.exitCode !== 0) {
    await setView($, { canForce: true })

    return fail('git push', pushed)
  }
  await saveConfig($, { selection: sel, lastBy: 'vous', lastAt: new Date().toISOString() })
  await setView($, { canForce: false, message: '' })

  return `Poussé sur ${branch} (${sel.length} élément${sel.length > 1 ? 's' : ''}).`
}

async function runPush($: $, isForced = false) {
  if ((await read($, viewAtom)).isBusy) {
    return
  }
  await setView($, { isBusy: true, status: 'Push en cours…' })
  const status = await push($, isForced).catch(err => `Erreur : ${String(err)}`)
  await setView($, { isBusy: false, status })
}

async function loadRepos($: $) {
  await setView($, { status: 'Chargement de tes dépôts GitHub…' })
  const r = await $.process
    .run(['gh', 'repo', 'list', '--limit', '100', '--json', 'url', '-q', '.[].url'], { timeoutMs: 60_000 })
    .catch(() => null)
  if (!r || r.exitCode !== 0) {
    await setView($, { status: 'gh indisponible ou non connecté (lance « gh auth login »).' })

    return
  }
  const repos = r.stdout.split('\n').map(s => s.trim()).filter(Boolean)
  await setView($, { repos, status: `${repos.length} dépôts trouvés.` })
}

async function loadBranches($: $) {
  const cfg = await read($, cfgAtom)
  if (!cfg?.remote) {
    return
  }
  const r = await $.process
    .run(['git', 'ls-remote', '--heads', cfg.remote], { timeoutMs: 60_000 })
    .catch(() => null)
  const branches = (r?.exitCode === 0 ? r.stdout : '')
    .split('\n')
    .map(line => line.split('refs/heads/')[1]?.trim())
    .filter((b): b is string => Boolean(b))
  await setView($, { branches })
}

async function setRemote($: $, url: string) {
  const remote = url.trim().replace(/\/$/, '')
  if (!remote) {
    return
  }
  await saveConfig($, { remote: remote.endsWith('.git') ? remote : `${remote}.git` })
  await setView($, { status: `Dépôt : ${remote}`, branches: [] })
  await loadBranches($)
}

async function createRepo($: $) {
  const view = await read($, viewAtom)
  const name = view.repoName.trim()
  if (!name) {
    await setView($, { status: 'Donne un nom au nouveau dépôt.' })

    return
  }
  await setView($, { isBusy: true, status: `Création de ${name}…` })
  const r = await $.process
    .run(['gh', 'repo', 'create', name, view.isPublic ? '--public' : '--private'], { timeoutMs: 60_000 })
    .catch(err => ({ exitCode: 1, stdout: '', stderr: String(err) }))
  if (r.exitCode !== 0) {
    await setView($, { isBusy: false, status: `Échec : ${r.stderr.trim()}` })

    return
  }
  const url = r.stdout.trim().split('\n').pop() ?? ''
  await saveConfig($, { branch: 'main' })
  await setRemote($, url)
  await setView($, { isBusy: false, repoName: '', status: `Dépôt créé : ${url}` })
}

// Après un `git push` lancé par Claude, on retient ce qu'il a poussé.
async function recordClaudePush($: $) {
  const files = await git($, ['ls-files'])
  if (files.exitCode !== 0) {
    return
  }
  const remote = await git($, ['remote', 'get-url', 'origin'])
  const branch = await git($, ['branch', '--show-current'])
  const selection = files.stdout.split('\n').map(s => s.trim()).filter(Boolean)
  const patch: Partial<ProjectConfig> = { selection, lastBy: 'Claude', lastAt: new Date().toISOString() }
  if (remote.exitCode === 0) {
    patch.remote = remote.stdout.trim()
  }
  if (branch.stdout.trim()) {
    patch.branch = branch.stdout.trim()
  }
  await saveConfig($, patch)
  await update($, selAtom, () => selection)
}

const open = async ($: $) => {
  await loadConfig($)
  await setView($, { isOpen: true, status: '' })
}

export const register: Register = on => {
  on('session.start', async ($, e, next) => {
    await $.command.register({ name: 'push-projet', description: 'Ouvre la fenêtre pour pousser le projet sur GitHub' })
    await $.command.register({
      name: 'push-rapide',
      description: 'Pousse tout de suite avec le dépôt, la branche et les fichiers du dernier push',
    })
    await loadConfig($)

    return next(e)
  })

  on('command.run', { command: 'push-projet' }, async $ => {
    await open($)

    return { text: 'Fenêtre GitPush ouverte au-dessus de la saisie.' }
  })

  on('command.run', { command: 'push-rapide' }, async $ => {
    await loadConfig($)

    return { text: await push($).catch(err => `Erreur : ${String(err)}`) }
  })

  on('tool.call', async ($, e, next) => {
    const result = await next(e)
    const command = 'command' in e && typeof e.command === 'string' ? e.command : ''
    if (/\bgit\b[^\n]*\bpush\b/.test(command) && !result.isError) {
      await recordClaudePush($).catch(() => undefined)
    }

    return result
  }).catch(($, e, next) => next(e))

  on('ui.render', { component: 'AbovePrompt' }, async ($, e, next) => {
    const view = await read($, viewAtom)
    if (!view.isOpen || e.props.hasSurvey) {
      return next(e)
    }
    if (e.surface === 'mobile') {
      const { Text } = $.ui.resolve(e)

      return <Text>GitPush : ouvre-le sur ordinateur, ou lance /push-rapide.</Text>
    }
    const { Box, Text, Button, Input, Select } = $.ui.resolve(e)
    const cfg = await read($, cfgAtom)
    const sel = await read($, selAtom)
    const cwd = await $.session.cwd()
    const branch = cfg?.branch || 'main'
    const repoShort = cfg?.remote ? cfg.remote.replace(/^https:\/\/github\.com\//, '').replace(/\.git$/, '') : ''
    const width = Math.max(30, e.props.bodyColumns - 4)
    const statusColor = { info: 'suggestion', ok: 'success', erreur: 'error' } as const
    const statusIcon = { info: '…', ok: '✓', erreur: '✗' }
    const mark = { oui: '☑', partiel: '◪', non: '☐' }
    const plural = sel.length > 1 ? 's' : ''

    const header = (
      <Box flexDirection="row" justifyContent="space-between">
        <Text weight="bold" color="claude">
          ⬆ GitPush <Text color="text">· {cwd.split(/[\\/]/).pop()}</Text>
        </Text>
        <Button key="close" plain dimColor label="✕ Fermer" role="dismiss" onPress={() => setView($, { isOpen: false })} />
      </Box>
    )

    const lastPush = cfg?.lastBy
      ? `pré-coché : dernier push de ${cfg.lastBy}${cfg.lastAt ? `, ${new Date(cfg.lastAt).toLocaleString('fr-FR', { dateStyle: 'short', timeStyle: 'short' })}` : ''}`
      : 'aucun push enregistré'
    const info = (
      <Box flexDirection="row" gap={2} flexWrap="wrap">
        <Text>
          <Text color="subtle">dépôt </Text>
          {repoShort ? <Text weight="semibold">{repoShort}</Text> : <Text color="warning">non configuré</Text>}
        </Text>
        <Text>
          <Text color="subtle">branche </Text>
          <Text weight="semibold" color="merged">⎇ {branch}</Text>
        </Text>
        <Text color="subtle">{lastPush}</Text>
      </Box>
    )

    const actions = (
      <Box flexDirection="row" gap={1} alignItems="center">
        <Box flexGrow={1}>
          <Input
            key="message"
            placeholder="Message du commit (optionnel)"
            value={view.message}
            onInput={message => setView($, { message })}
            submitLabel="Pousser"
            onSubmit={async message => {
              await setView($, { message })
              await runPush($)
            }}
          />
        </Box>
        <Button
          key="push"
          variant="primary"
          label={view.isBusy ? 'Push en cours…' : `⬆ Pousser ${sel.length} élément${plural} → ${branch}`}
          onPress={() => runPush($)}
        />
        {view.canForce && <Button key="force" label="Forcer" onPress={() => runPush($, true)} />}
      </Box>
    )

    const tabs = (
      <Box flexDirection="row" gap={1}>
        <Button
          key="tab-files"
          variant={view.tab === 'fichiers' ? 'primary' : 'secondary'}
          label={`Fichiers (${sel.length})`}
          onPress={() => setView($, { tab: 'fichiers' })}
        />
        <Button
          key="tab-repo"
          variant={view.tab === 'depot' ? 'primary' : 'secondary'}
          label="Dépôt & branche"
          onPress={async () => {
            await setView($, { tab: 'depot' })
            await loadBranches($)
          }}
        />
      </Box>
    )

    let body
    if (view.tab === 'fichiers') {
      const rows = await buildRows($, view.expanded)
      body = (
        <Box flexDirection="column">
          <Box flexDirection="row" gap={2}>
            <Button key="all" plain dimColor label="Tout cocher" onPress={() => update($, selAtom, () => rows.filter(r => r.depth === 0).map(r => r.path))} />
            <Button key="none" plain dimColor label="Tout décocher" onPress={() => update($, selAtom, () => [])} />
            <Button key="last" plain dimColor label="↺ Dernier push" onPress={() => update($, selAtom, () => cfg?.selection ?? [])} />
          </Box>
          {rows.map(row => {
            const state = checkState(sel, row.path)
            const isExpanded = view.expanded.includes(row.path)

            return (
              <Box key={`row:${row.path}`} flexDirection="row" paddingLeft={row.depth * 2}>
                {row.isDir ? (
                  <Button
                    key={`open:${row.path}`}
                    plain
                    dimColor
                    label={isExpanded ? '▾' : '▸'}
                    onPress={() =>
                      setView($, {
                        expanded: isExpanded
                          ? view.expanded.filter(p => p !== row.path && !isUnder(p, row.path))
                          : [...view.expanded, row.path],
                      })
                    }
                  />
                ) : (
                  <Text> </Text>
                )}
                <Button key={`check:${row.path}`} plain onPress={() => toggle($, row.path)}>
                  <Text color={state === 'non' ? 'inactive' : 'success'}>{mark[state]} </Text>
                  <Text color={state === 'non' ? 'inactive' : 'text'} weight={row.isDir ? 'semibold' : 'regular'}>
                    {row.isDir ? `${row.name}/` : row.name}
                  </Text>
                </Button>
              </Box>
            )
          })}
          {rows.length >= MAX_ROWS && <Text color="subtle">… liste tronquée</Text>}
        </Box>
      )
    } else {
      body = (
        <Box flexDirection="column" gap={1}>
          <Text weight="semibold">Dépôt</Text>
          <Input key="remote" label="URL" placeholder="https://github.com/moi/projet.git" value={cfg?.remote ?? ''} submitLabel="Utiliser" onSubmit={url => setRemote($, url)} />
          {view.repos.length > 0 ? (
            <Select
              key="repo-pick"
              label="Mes dépôts GitHub"
              options={view.repos.map(url => ({ value: url, label: url.replace('https://github.com/', '') }))}
              value={cfg?.remote.replace(/\.git$/, '')}
              onSelect={url => setRemote($, url)}
            />
          ) : (
            <Button key="repos" label="Choisir parmi mes dépôts GitHub" onPress={() => loadRepos($)} />
          )}
          <Text weight="semibold">Branche</Text>
          {view.branches.length > 0 && (
            <Select key="branch-pick" label="Existante" options={view.branches.map(b => ({ value: b }))} value={branch} onSelect={b => saveConfig($, { branch: b })} />
          )}
          <Input key="branch" label="Nouvelle" placeholder={branch} submitLabel="Utiliser" onSubmit={b => b.trim() && saveConfig($, { branch: b.trim() })} />
          <Text weight="semibold">Créer / recréer un dépôt GitHub</Text>
          <Box flexDirection="row" gap={1} alignItems="center">
            <Box flexGrow={1}>
              <Input
                key="repo-name"
                placeholder="nom-du-depot"
                value={view.repoName}
                onInput={repoName => setView($, { repoName })}
                submitLabel="Créer"
                onSubmit={async repoName => {
                  await setView($, { repoName })
                  await createRepo($)
                }}
              />
            </Box>
            <Select
              key="visibility"
              options={[{ value: 'prive', label: 'Privé' }, { value: 'public', label: 'Public' }]}
              value={view.isPublic ? 'public' : 'prive'}
              onSelect={v => setView($, { isPublic: v === 'public' })}
            />
            <Button key="create" label="Créer" onPress={() => createRepo($)} />
          </Box>
          <Button
            key="forget"
            plain
            dimColor
            label="Oublier la config de ce projet"
            onPress={async () => {
              await $.store.delete(storeKey(cwd))
              await update($, cfgAtom, () => null)
              await update($, selAtom, () => [])
              await setView($, { status: 'Config effacée.' })
            }}
          />
        </Box>
      )
    }

    return (
      <Box flexDirection="column" borderStyle="round" borderColor="claude" paddingX={1}>
        {header}
        {info}
        {actions}
        {view.status && (
          <Text color={statusColor[view.statusKind]}>
            {statusIcon[view.statusKind]} {view.status}
          </Text>
        )}
        <Text color="subtle">{'─'.repeat(width)}</Text>
        {tabs}
        {body}
      </Box>
    )
  })
}
