# Plan — choisir son harness à l'installation

État au 2026-10-30, branche `hardening/dashboard-and-fork` (commit de fondation
`8247447`).

## Le besoin

`base install` écrit `~/.claude/settings.json` et appelle ça « câblé ». Trois
problèmes :

1. Un utilisateur sans Claude Code obtient une installation qui annonce un
   succès et ne fait rien.
2. Cursor lit `~/.claude/settings.json` par défaut (imports tiers), donc Cursor
   est *déjà* à moitié supporté — par accident de compatibilité, pas par choix.
3. Rien dans l'installation ne demande à l'utilisateur pour quel(s) agent(s) il
   veut base.

Objectif : à l'installation, choisir explicitement parmi les quatre harness
opérationnels — **Claude Code, OpenCode, Cursor, Copilot CLI** —, avec un défaut
sûr quand personne ne répond.

## Ce qui existe déjà (fondation, commit `8247447`)

- `src/harness/mod.rs` : enum `Harness` (4 variantes), `canonical_event`,
  `inbound`/`outbound`, `context_of`, trait `Binding`, `WireOutcome`,
  `registry()`, `wire_detected()`, `unwire_all()`.
- `src/harness/claude.rs` : binding complet, délègue à l'étape 3 de
  `base install` (chemin différé vers l'étape 8).
- `src/harness/opencode.rs`, `cursor.rs`, `copilot.rs` : stubs avec détection et
  chemins réels, `wire`/`unwire` en échec explicite.
- `base hook <event> --harness <host>` : la charge utile de l'hôte est normalisée
  à l'entrée, la sortie du moteur est ré-emballée pour l'hôte.
- `base install --harness <host>|all` et `base uninstall --harness` : un seul
  nom pour l'instant.
- `hook::dispatch` collecte dans un `String` au lieu d'imprimer, pour que
  `outbound` puisse emballer l'émission entière.

## Étape 1 — La sélection à l'installation

### Surface CLI

```text
base install                                  # interactif si TTY : on demande
base install --harness opencode               # un seul
base install --harness claude,opencode        # plusieurs
base install --harness all                    # tous ceux détectés
base install --harness all --skip-hooks       # rien n'est câblé
base install --no-harness-prompt              # non interactif : tous ceux détectés
base uninstall --harness cursor               # symétrique
```

- `--harness` accepte désormais une **liste séparée par des virgules**, en plus
  de `all` / `auto` (= tous ceux détectés) et des quatre noms.
  `Harness::parse_list(&str) -> Option<Vec<Harness>>` dans `src/harness/mod.rs`.
- Un nom inconnu sort en erreur avec la liste des noms valides (jamais de repli
  silencieux sur « tous »).

### L'invite (TTY seulement)

Après l'étape 2 de l'install, avant l'étape 3 :

```text
Which agents should base attach to?
  1. Claude Code   detected  ~/.claude
  2. OpenCode      detected  ~/.config/opencode
  3. Cursor        not found
  4. Copilot CLI   not found

Enter numbers (1,2), `a` for every detected, `n` for none [default: 1,2]:
```

- Défaut = **tous ceux détectés**, jamais « les quatre » : câbler Cursor sur une
  machine sans Cursor produit un fichier que personne ne lit.
- `n` = aucune liaison ; l'install continue et le dit explicitement
  (`⊘ No harness wired — base will not run inside any agent`).
- Pas de TTY, ou `--no-harness-prompt`, ou `--harness` fourni : aucune question.
  Une installation scriptée doit être déterministe.
- La réponse est récapitulée dans le transcript, une ligne par harness, avec le
  fichier écrit et le nombre d'entrées ajoutées.

### Détection

| Harness | Indice retenu | Chemin câblé |
|---|---|---|
| Claude Code | `~/.claude/` | `~/.claude/settings.json` |
| OpenCode | `~/.config/opencode/` | `~/.config/opencode/plugins/base.ts` |
| Cursor | `~/.cursor/` | `~/.cursor/hooks.json` |
| Copilot CLI | `$COPILOT_HOME` ou `~/.copilot/` | `~/.copilot/hooks/base.json` |

La détection est un indice, pas une preuve : `--harness <nom>` câble même un
hôte non détecté (l'utilisateur sait ce qu'il fait), et `base install` relancé
plus tard rattrape un hôte installé après base.

## Étape 2 — Les trois adaptateurs

Chacun est un fichier, un commit, et son propre fichier de test. Aucun ne touche
les fichiers partagés (`src/harness/mod.rs`, `src/cli.rs`, `src/install.rs`,
`src/lib.rs`, `Cargo.toml`, `.github/test-counts.txt`, références CLI générées).

### 2.1 OpenCode — `src/harness/opencode.rs`

- `wire` écrit `~/.config/opencode/plugins/base.ts` : un plugin 2.x
  (`Plugin.define({ id, setup })`, `node:child_process` `execFile`, pas `$`).
- Le plugin court-circuite sur `BASE_HEADLESS`, a un timeout, avale toute erreur,
  et mappe : `session.hook("prompt")` → `user-prompt-submit`,
  `session.hook("context")` → `session-start` (une fois par session) puis le
  contexte d'outil, `tool.hook("execute.before"/"after")` → `pre/post-tool-use`,
  `session.idle` → `stop`, `session.deleted` → `session-end`.
- Marqueur en tête de fichier ; `unwire` ne supprime que ce qui porte le
  marqueur, un `base.ts` utilisateur n'est jamais écrasé.
- Tests : contenu généré (marqueur, `--harness opencode`, les six événements,
  pas de `$(`), idempotence, `unwire` absent/utilisateur, `outbound` sur une
  ligne avec `additionalContext`.

C'est le seul binding testable de bout en bout sur cette machine.

### 2.2 Cursor — `src/harness/cursor.rs`

- Fusion **insert-only, octet pour octet** dans `~/.cursor/hooks.json` (le
  fichier contient déjà rtk et ponytail) : insertion texte + preuve par
  re-parse « original + exactement ces entrées », comme `src/settings_json.rs`.
- `unwire` = l'édition inverse exacte ; round-trip octet-identique testé.
- **Garde anti-double-injection** : si `~/.claude/settings.json` contient déjà
  `base hook`, Cursor les importe par défaut → ne pas câbler nativement, le dire
  dans la ligne d'install.
- `inbound` : `Shell` → `Bash`, `Write` → `Edit`, inconnu passe tel quel,
  `cwd`/`session_id` préservés. Pas de `Glob`.
- `outbound` : une forme par événement, vérifiée dans la doc Cursor
  (`additional_context` pour le prompt, enveloppe `hookSpecificOutput` pour les
  outils), sur une ligne, enveloppe déballée via `context_of`.

### 2.3 Copilot CLI — `src/harness/copilot.rs`

- `wire` écrit `~/.copilot/hooks/base.json` (ou `$COPILOT_HOME`), un fichier
  propre à base : `{"version":1,"hooks":{…}}`, `bash` + `powershell` +
  `timeoutSec` par événement, six événements.
- `unwire` supprime le fichier seulement si **toutes** ses commandes nomment
  `base hook` ; sinon il retire les entrées de base et laisse le reste.
- `inbound` : `sessionId` → `session_id`, `toolName` → `tool_name`,
  `toolArgs` → `tool_input` (chaîne JSON acceptée), tout le reste préservé.
- `outbound` : `{"additionalContext": …}` sur une ligne.
- Injection-only : les bugs upstream ouverts (#2142, #2491, #2013) interdisent de
  dépendre de `sessionStart`. Le commentaire de module le dit.

## Étape 3 — Test de conformité partagé

`tests/harness_conformance_test.rs` + `tests/fixtures/harness/<hôte>/` :

- une charge utile par hôte, même événement → même texte injecté pour les quatre ;
- aller-retour `base hook <event> --harness <hôte>` : fixture sur stdin, JSON
  attendu sur stdout ;
- `cwd` et `session_id` survivent à la normalisation (le tier du workspace en
  dépend).

C'est ce test qui empêche quatre transports de diverger.

## Étape 4 — Doctor, docs, compteurs

- `base doctor` : section harness — détecté / câblé / binaire sur `PATH` /
  dernière erreur de hook.
- `docs/harnesses.md` : matrice et statut réels (ce qui est testé, ce qui ne
  l'est pas).
- `BASE_REGEN_DOCS=1 cargo test --bin base help_docs`, puis
  `.github/test-counts.txt` réconcilié sur la suite complète.

## Étape 5 — Backend LLM (après les adaptateurs)

`src/llm.rs` lance `claude -p` en dur. Sans Claude Code, `base graph extract`,
`base graph query`, `base tune` et le juge des corrections sont morts.

- `[llm] backend = "auto" | "claude" | "opencode" | "cursor" | "copilot"` dans
  `base.toml` ; `auto` sonde `PATH` dans cet ordre.
- Chaque backend reçoit `BASE_HEADLESS=1` (sinon ses hooks se rappellent).
- Le backend OpenCode exige un agent dédié **sans outils** : `opencode run` par
  défaut est un agent de codage complet, lent et dangereux pour une extraction
  de fond.

## Ordre d'exécution

| # | Tâche | Commit |
|---|---|---|
| 1 | `parse_list` + invite interactive + `--no-harness-prompt` | `feat(install): choose the harness` |
| 2 | Adaptateur OpenCode + tests | `feat(harness): OpenCode plugin and binding` |
| 3 | Adaptateur Cursor + tests | `feat(harness): Cursor binding` |
| 4 | Adaptateur Copilot + tests | `feat(harness): Copilot CLI binding` |
| 5 | Conformité partagée + doctor + docs | `test(harness): conformance` |
| 6 | Backend LLM | `feat(llm): backend abstraction` |

## Portes de sortie

- `cargo test` complet vert (le test `reminder_unarchive_restores` est instable
  après 23 h, préexistant).
- `cargo clippy --all-targets -- -D warnings` propre.
- Install deux fois → `changed: 0` ; install puis uninstall → fichier
  octet-identique à l'original pour chaque hôte.
- Rien n'est poussé sans que `release-key.pub` ait été généré : le workflow de
  release échoue volontairement tant que la clé est le placeholder `UNSET`.

## Risques connus

- **API OpenCode mouvante** (2.1.4 locale) : le plugin est validé par inspection
  et tests de contenu, pas par une session réelle pilotée ici.
- **Cursor** peut changer son chemin de compatibilité Claude ; le chemin natif
  est celui qu'on teste, la compat est un bonus.
- **Copilot** : aucun binaire sur cette machine ; tout est vérifié contre la doc
  et les fixtures.
- **Limite d'usage des sous-agents** : les trois worktrees
  (`harness-opencode`, `harness-cursor`, `harness-copilot`) sont créés sur
  `8247447`, mais la génération parallèle a échoué sur la limite du modèle
  gratuit. Les adaptateurs s'implémentent donc séquentiellement, un commit par
  adaptateur ; les worktrees restent en place pour une nouvelle tentative.
