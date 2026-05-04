# Qmonster UI 사용자 매뉴얼

현재 Qmonster TUI는 **상단 Alerts**, **하단 Panes**, **footer**, 그리고
필요할 때 열리는 **overlay**들로 구성됩니다. 이 문서는 현재 구현된
표기와 조작만 설명합니다.

## 1. 화면 구성

- **Alerts**: 현재 target 기준의 경고/추천 큐입니다. 제목에는
  `visible`, `new`, `auto-hide` 개수가 표시됩니다.
- **Panes**: 선택된 session/window 안의 pane 목록입니다. 현재 선택된
  pane는 같은 리스트 안에서 상세 내용이 아래로 펼쳐집니다.
- **Alerts/Panes divider**: Alerts와 Panes 사이의 한 줄 divider를
  드래그해 두 영역의 높이를 조절할 수 있습니다. 키보드에서는 `[` / `]`
  로 Alerts 영역을 줄이거나 키우고, `/`로 split 비율을 한 단계씩
  순환하며, `=`로 기본 비율로 되돌립니다.
- **Footer**: 현재 focus, Alerts/Panes split 비율, 주요 조작 키를 보여줍니다.
- **Overlay**: `t`로 target picker, `S`로 settings, `?`로 help,
  footer 오른쪽 아래 버전 배지를 클릭하면 Git overlay가 열립니다.

## 2. Alerts 읽는 법

- Alerts는 **심각도 우선**으로 정렬됩니다.
  같은 심각도 안에서는 `NEW`가 먼저, 그 다음 최신 시각이 먼저 옵니다.
- 각 항목 첫 줄은 대략 다음 형태입니다.

```text
[14:23:08] NEW  WARNING  Checkpoint · %56
```

- 첫 줄 아래에는 항상 `dismiss` 줄이 옵니다.
  평소에는 `[ ] click hide · Enter/Space hide`,
  숨김 예약 상태에서는 `[x] auto-hide in Ns · click undo · Enter/Space undo`
  로 보입니다.
- 현재 숨김 예약 유지 시간은 기본 **20초**입니다.
- 그 아래에는 `summary`, 필요하면 `next`, `run`이 `label : value`
  정렬로 붙습니다.
- Alert 종류 제목은 현재 다음과 같이 나뉩니다.
  `System Notice`, `Checkpoint`, `Cross-Pane`, `Cross-Window`, 일반
  recommendation 제목. `Cross-Window`은 v1.17.0(Phase D D1)에서 추가된
  새 분류로, 동일 `current_path` + `git_branch` panes가 2개 이상의
  tmux window에 걸쳐 있을 때 `[security] cross_window_findings = true`
  opt-in 시 발화합니다.
- Alerts 맨 위 `bulk hide :` 줄의 severity chip은 **actionable alert만**
  대상으로 합니다. `c`로 지울 수 있는 system notice는 여기에 포함되지
  않습니다.

## 3. Panes 읽는 법

- pane 제목은 현재 다음 형태입니다.

```text
session:window · Provider role · %pane_id
```

- 예:
  `qmonster:0 · Codex review · %57`
- 각 pane에는 보통 다음 줄들이 붙습니다.
  `state`, `path`, `cmd`, `status`, `blocked`, `signals`, `metrics`,
  `modes`, `access`, `loaded`, `restrict`
- `state` 줄은 pane가 멈춤/대기 상태일 때 보입니다. 상태가 바뀐 직후에는
  약 3초 동안 `CHANGED` 배지와 pulse highlight가 붙고, active로 돌아온
  경우에도 짧게 `▶ ACTIVE` state 줄을 보여줍니다. 색만으로 상태 변화를
  알리지 않기 위해 텍스트 배지를 함께 사용합니다. 선택 여부와 무관하게
  변경된 카드 첫 줄은 `STATE CHANGED`로 시작하고, `state` 줄에는
  `CHANGED` 배지가 붙습니다. 선택 highlight 자체는 상태 변화 표시로
  쓰지 않으므로 선택된 카드와 선택되지 않은 카드의 변화 표시 규칙이 같습니다.
  선택된 카드에서도 상태 badge 색이 묻히지 않도록 selection highlight는
  상태 span 색/배경을 덮어쓰지 않고, 선택 표시는 첫 줄의 `▶` marker로만
  합니다. 따라서 펼쳐진 pane의 모든 줄에 underline이나 강조선을 반복해서
  그리지 않습니다.
  멈춤/대기 상태 배지(`IDLE`, `WAIT`, `USAGE LIMIT`)에는 경과 시간
  배지(`⏱ MM:SS` 또는 `H:MM:SS`)가 함께 표시됩니다. 또한 상태가 유지되는
  동안 pane 제목 앞에는 `IDLE DONE`, `IDLE STALE`, `WAIT INPUT`,
  `WAIT APPROVAL`, `USAGE LIMIT` 같은 지속 prefix가 high-contrast badge로
  남고, state 줄에는 `COMPLETE`, `STILL IDLE`, `INPUT NEEDED`,
  `APPROVAL NEEDED`, `ACTION REQUIRED` 같은 지속 marker가 붙습니다.
- `status`는 현재 `high confidence`, `medium confidence`,
  `low confidence`, `unknown confidence`처럼 텍스트로 표시됩니다.
  canonical pane title(`{provider}:{instance}:{role}`)은 High confidence로
  그대로 우선합니다. title이 없더라도 provider status surface가 구조적으로
  확인되면 Qmonster는 provider를 Medium confidence로 두고 기본 role을
  `main`으로 채웁니다. 운영자가 `review` / `research` 역할을 정확히
  구분하려면 pane title convention을 직접 설정해야 합니다.
- `blocked` 줄은 가장 중요한 대기 상태만 따로 보여줍니다.
  `waiting for input`, `approval needed`
- `signals` 줄은 그 외 상태를 보여줍니다.
  `log storm`, `repeated output`, `verbose output`, `error hint`,
  `subagent activity`
- `metrics` 줄은 badge 형태로 표시됩니다.
  `CTX 90%`, `QUOTA 5H 47%`, `QUOTA WEEK 62%`,
  `TOKENS 12345 [Official]`, `COST $0.42 [Estimate]`,
  `MODEL gpt-5.4 [Official]`
- `CTX` badge는 수치가 높을수록 더 강한 severity 색을 사용합니다.
  85% 이상은 `Risk`, 75% 이상은 `Warning`, 60% 이상은 `Concern`으로
  취급됩니다. `QUOTA`, `QUOTA 5H`, `QUOTA WEEK` badge도 같은 severity
  임계치를 공유합니다.
- 현재 `CTX`는 구조적으로 확인 가능한 provider status에서만 채웁니다.
  Claude는 live statusline의 `CTX N%`, Codex는 bottom status line,
  Gemini는 status table의 `context` 컬럼을 사용합니다. Claude `/clear`
  직후 statusline이 `CTX —`를 노출하면 Qmonster는 이전 CTX cache를
  재사용하지 않고 `CTX 0%`로 표시합니다. 과거 Claude `/context`
  capture overlay 파서는 호환용으로 남아 있지만, 운영 경로는
  statusline입니다.
- `QUOTA 5H`와 `QUOTA WEEK`는 Claude/Codex 전용 split quota입니다.
  Claude는 live statusline의 `5h N%`와 `7d N%`를 그대로 pressure로
  표시합니다. Codex는 bottom status line의
  `5h N%`와 `weekly N%`를 **남은 quota**로 읽고, Qmonster의 pressure
  badge에는 `100 - N` 값을 표시합니다. Gemini처럼 provider가 단일 quota만
  노출하는 경우에는 기존 `QUOTA N%` badge를 사용합니다.
- **Provider 측의 status surface는 운영자가 보이는 항목을 끌 수 있음**:
  Codex의 `/statusline` 슬래시 명령 ("Configure which items appear in
  the status line")은 bottom status line의 항목(branch / model / input
  / output / version 등)을 토글합니다. Gemini의 `/footer` (alias
  `/statusline`) 슬래시 명령은 footer/status table의 컬럼(`ui.footer.*`
  설정 — `hideCWD` / `hideSandboxStatus` / `hideModelInfo` /
  `hideContextPercentage` / `hideFooter`)을 토글합니다. 운영자가 항목을
  숨기면 Qmonster 파서는 해당 필드를 None으로 두며, 거짓 값을 추정해서
  채우지 않습니다 — 부재가 honesty (S3-4와 같은 원칙).
- `cmd` 줄은 tmux `pane_current_command` 값입니다. 예:
  `target/release/qmonster`, `codex`, `node`. 이 값은 provider/role
  식별과 별개로 “현재 pane이 무엇을 실행 중인지”를 보여주는 운영 힌트입니다.
- Codex bottom status line의 `1.51M in · 20.4K out` 토큰은 **세션
  누적값**입니다 (Codex `TokenUsage` 구조에서 `input_tokens` /
  `output_tokens` 필드 — 검증됨). Qmonster는 이를 `SignalSet.input_tokens`
  / `output_tokens`로 노출합니다. metric badge는 여전히 compact summary인
  `TOKENS`(total)를 표시하고, 선택된 pane 상세에는 두 값이 모두 있을 때
  `token io: Main 1.51M in / 20.4K out [Official]` 형태의 breakdown을
  추가로 보여줍니다. **Subagent token 분리는 영구 deferred**입니다 —
  Claude / Codex / Gemini 모두 per-subagent input/output 카운터를
  노출하지 않으므로, 세션 누적값은 subagent 작업까지 이미 포함합니다.
  Phase D D3-A (v1.19.0)은 *탐지*만 정밀화했습니다: Claude `● Task(`
  tail signature가 `subagent_hint`를 발화하고, 일반 tool 호출
  (`● Bash(...)`, `● Read(...)`)나 TODO 프로즈 (`Task 1 — ...`)는
  발화하지 않습니다. token attribution은 provider가 구조화된 카운터를
  내놓을 때까지 시도하지 않습니다 (ARCHITECTURE.md "Deferred for later
  phases" 참고).
- `MODEL` badge는 source가 있을 때만 표시합니다. Claude pane은
  `~/.claude/settings.json`에 `"model"` 키가 있을 때만 채워지므로,
  사용자 환경이 그 키를 비워둔 상태(=Claude Code가 기본 모델
  선택을 동적으로 하는 상태)에서는 의도적으로 빈칸으로 둡니다.
  허위 표시(예: `claude --version` 결과를 모델 이름으로 둔갑)
  대신 부재가 곧 honesty라는 전제입니다 (S3-4 design decision (b)).
  Codex / Gemini는 status surface에서 직접 `gpt-…` / `gemini-…`
  토큰을 읽을 수 있으면 채웁니다. Codex는 `gpt-5.5 xhigh`처럼
  trailing model-with-reasoning status item에만 effort가 붙는 경우에도
  `MODEL`과 `EFFORT`를 분리해 채웁니다.
- `MEM` badge shows process resident memory in MiB. Sources by provider:
  - Gemini: status-table `memory` column (`118.8 MB` / `1.2 GB`) — `[Official]`.
  - Claude / Codex: F-1 reads `/proc/<descendant>/status` VmRSS for the
    highest-RSS descendant of the pane's foreground shell PID (depth ≤ 5,
    with a visited-set guarding against diamonds/cycles), preferring
    descendants whose `comm` matches the `KNOWN_CLI_COMMS` allowlist (`claude` / `codex` / `gemini` / `node` /
    `python` / `python3`) — `[Heur]`. If `/proc` is unreadable, no
    descendant exists, or `pane_pid` is `None`, the badge stays absent
    (honesty rule).
- `MEM-FILE` badge shows total bytes of provider-specific agent
  memory files: Claude sums `CLAUDE.md` (project root + `~/.claude/`)
  plus all `.md` files in `~/.claude/projects/<encoded>/memory/`;
  Codex sums `AGENTS.md` (project root + `~/.codex/`) plus
  `~/.codex/AGENTS.override.md`; Gemini sums `GEMINI.md` (project
  root + `<project>/.gemini/`) plus `~/.gemini/GEMINI.md`. Per-file
  size is capped at 1 MiB during the scan to prevent a pathological
  file from dominating the total. Source label is always `[Heur]`
  because file existence is not proof the CLI loaded the bytes — it
  is an observation that the bytes are _available_ to be loaded.
  Format is `<N> KB` between 1 KiB and 1 MiB and `<N.N> MB` at or
  above 1 MiB; sub-1 KiB renders as `<1 KB` so operators can
  distinguish "tiny non-zero" from "no files found" (badge omitted
  in the latter case — honesty rule). Total above 50_000 bytes
  (~49 KiB) triggers a `memory_bloat_advisory` Concern recommendation
  pointing the operator toward `.claude/skills/`,
  `~/.codex/AGENTS.override.md`, or `.gemini/skills/` on-demand
  files instead of `/compact` / `/clear` / `/memory`.
- `TOKENS` sparkline appears near the top of the SELECTED pane card
  (Tab to panes focus, then ↑/↓ to select), before recommendation
  details so it is not pushed off-screen by long alerts. It shows the
  delta in prompt tokens (`input_tokens + cached_input_tokens`) between adjacent recent samples (last 20
  polls fetched DESC; rendered oldest-to-newest left-to-right), mapped
  to the 8-block Unicode set `▁▂▃▄▅▆▇█`. It is rendered as plain
  high-contrast text, not a background badge, so the thin block glyphs
  stay legible. Idle pane → all lowest-block; active pane → rising
  blocks. The metric measures rate of
  context-prompt growth, NOT cumulative usage; samples themselves are
  persisted as cumulative counts. When fewer than 2 samples have been
  recorded for the pane, the selected card shows `TOKENS collecting
N/2` instead of staying blank. Token-source providers today: Codex
  (bottom-status `1.51M in / 20.4K out` and `/status` token usage),
  Claude sidefile (`input_tokens` / `output_tokens` / cache reads),
  and Gemini `/stats model` after the operator cycles `u`.
- `CACHE` badge shows the cache hit ratio for the pane's cumulative
  prompt input — `cache_hit_ratio = cached_input_tokens /
(input_tokens + cached_input_tokens) × 100`, formatted with one
  decimal. Source label tracks `cached_input_tokens.source_kind`
  (`[Official]` for Codex `/status`, Claude sidefile/statusline cache,
  and Gemini `/stats model` when Cache Reads is visible). The badge
  appears only when `cached_input_tokens` or a provider cache ratio is
  `Some(...)`; Gemini OAuth keeps it absent because the Cache Reads row
  is not exposed. Format: `cache <N.N>%` (text) or `CACHE <N.N>%`
  (TUI). When selected-pane details have raw cache counts, Qmonster
  also shows `cache io: read <N> / create <N>`; `create` is currently
  Claude sidefile `cache_creation_input_tokens` and is not folded into
  the hit-ratio badge.
  Codex example: `Token usage: total=210,058 input=189,703 (+ 1,317,376
cached) output=20,355` → `CACHE 87.4% [Official]` (1,317,376 of
  1,507,079 prompt-input tokens were cache-hits, ~87% reuse).
- v1.27.1 parser fix: Codex `Token usage:` lines are parsed atomically
  into total/input/cached/output token counts. When a resumed session
  also shows a footer placeholder like `0 in · 0 out`, the official
  `Token usage:` values continue to drive the token row and `CACHE`
  badge.
- Cache-aware advisory recommendations (Phase F F-7, v1.26.0):
  When the `CACHE` badge crosses the hot threshold (60%) while
  context still has headroom (< 70% used), Qmonster surfaces a
  `Concern` `cache: avoid /compact while cache is hot` recommendation
  that explains running `/compact` would reset cache and force a
  full prompt rebuild. Wait until ctx >= 80% so the cache rebuild
  cost amortizes over more turns. Conversely, when the cache hit
  ratio drops below 30% while context is filling (> 60% used), a
  `Good` `cache: /compact is safe — cache is cold` recommendation
  fires with `suggested_command: /compact` — the cache rebuild cost
  is already paid on every turn, so compacting won't cost cache
  effectiveness. Both recommendations gate on
  `IdentityConfidence ≥ Medium` and suppress when input/permission
  wait is active. The default thresholds listed here are configurable
  through `[cache]`.
- **Third rule (F-7b, v1.27.0)**: `cache: drift detected — /compact will let cache rebuild` fires `Severity::Concern` with `suggested_command: /compact` when the cache hit ratio has dropped by ≥ 30 percentage points between the oldest sample in the recent window (last 20 polls) and the current SignalSet, AND at least 4 samples are available. The reason text reports the actual drop, baseline, and current values. Use this signal when context drifted (e.g., after a long agent-driven exploration) and the cache prefix has lost its alignment with the current prompt — `/compact` will trim the surface so cache rebuilds quickly on the next turn. Same suppression conditions as the other two cache rules.
- **Operator-tunable thresholds (F-7-config, v1.28.0)**: all 6 cache-rule
  thresholds are now configurable via the `[cache]` section in
  `~/.qmonster/config/qmonster.toml`. Defaults match the prior
  hardcoded values exactly:
  - `hot_ratio_threshold = 0.6` — `cache_hot_compact_warning` fires
    above this ratio
  - `cold_ratio_threshold = 0.3` — `compact_when_cache_cold` fires
    below this ratio
  - `hot_low_ctx_threshold = 0.7` — hot warning requires
    context_pressure below this
  - `cold_high_ctx_threshold = 0.6` — cold compact-recommendation
    requires context_pressure above this
  - `drift_drop_threshold = 0.30` — `cache_drift_compact` fires when
    cache_hit_ratio drops by ≥ this absolute amount
  - `drift_min_samples = 4` — `cache_drift_compact` requires this
    many samples in the recent window
    Adjust to taste. The `S` settings overlay does not yet edit `[cache]`
    — operators modify `qmonster.toml` directly and Qmonster picks up
    changes on the next config load.
- 긴 worktree 경로 문자열은 PATH badge에서 40자까지 자동
  ellipsize됩니다 (Slice 3 housekeeping). 잘린 부분은 `…` 한 글자로
  표시되어 badge 한 줄이 pane card 폭을 넘기지 않습니다.
- `modes` / `access` / `loaded` / `restrict` 줄은 provider runtime fact를
  표시합니다. Claude는 statusline에서 모델/effort/path와
  `⏵⏵ bypass permissions on/off`를 읽습니다. 선택된 Claude pane에서 `u`를
  눌러도 slash command나 `Escape`를 보내지 않고 다음 poll만 당겨옵니다.
  Codex와 Gemini는 선택된 pane에서 `u`를 누르면 provider의 read-only runtime
  slash command와 terminal submit(`C-m`, Enter-equivalent)을 보냅니다:
  Codex `/status`, Gemini idle/stale/limit-hit pane에서는 `/model` →
  `/stats session` → `/stats model`, active pane에서는 `/stats session` →
  `/stats model`.
  Gemini `/stats ...` 명령은 pre-`Escape` 없이 순환하지만, `/model`은
  picker 화면을 열기 때문에 Qmonster가 필요한 tail을 캡처한 뒤 `Escape`로
  한 번 닫아 다음 `u` cycle 명령이 바로 실행 가능하게 합니다.
  `/model`의 `Reset:` / `Resets:` 모델별 행은 `limits:` runtime fact와
  `metrics:` `RESET` 배지로 표시합니다. 현재 status table의 `/model`
  값이 `gemini-3.1-pro-preview`이면 `Pro`로 시작하는 reset 행만 남기고,
  `Flash` / `Flash Lite` 행은 표시하지 않습니다. 닫힌 picker 화면은 live
  scrollback에 남지 않으므로, 이 reset 캡처는 다음 `/model` 캡처나 pane
  lifecycle reset 전까지 유지되어 다음 poll에서 배지가 사라지지 않습니다.
  `/stats tools`는 현재 Qmonster가 파싱하는 표출 항목이 없어 보내지
  않습니다. `thinking...`
  진행 표시가 있으면 tail이 몇 poll 동안 같아도 `IDLE`로 떨어지지 않고,
  live prompt가 남아 있어도
  최근 tail이 변하는 동안은 active로 유지됩니다. 다음 poll에서
  캡처와 읽을 수 있는 로컬 provider 설정을 `RuntimeFact`로 파싱합니다.
  Claude `/btw`는 작업 중에도 즉시 실행되지만 도구/내부 상태 접근이 없는
  side question이라 runtime fact source로 쓰지 않습니다.
  예: `PERM`, `MODE`, `SANDBOX`, `DIR`, `AGENTS`, `TOOL`, `SKILL`,
  `PLUGIN`.
- 이 줄들은 “보였다”가 아니라 “provider status/config source에서 확인된”
  값만 보여줍니다. 해당 provider가 특정 값(예: 전체 tool registry나
  active skill list)을 slash/status로 노출하지 않으면 Qmonster는 값을
  꾸며내지 않고 빈 줄로 둡니다.
- 기본값에서는 YOLO / bypass permissions / Full Access /
  `danger-full-access` / `no sandbox`도 위 runtime badge로만 표시합니다.
  운영자가 `~/.qmonster/config/qmonster.toml`의
  `[security] posture_advisories = true`를 켜면 같은 관측값이
  `security-posture: review permissive runtime` Concern recommendation으로
  승격됩니다. 이 advisory는 passive이며 Notify를 울리지 않습니다.
- 선택된 pane는 recommendation과 provider profile payload를 아래로
  펼쳐서 보여줍니다.
- 선택된 (펼쳐진) pane 카드에 pending prompt-send proposal이 있으면
  카드 상세 영역에 `proposal:` 한 줄이 표시됩니다.
  예: `proposal: /compact  → press p to accept · d to reject`.
  `p` / `d`로 작용하기 전에 사전 explainer 모달이 뜨며, 빈도는
  `~/.qmonster/config/qmonster.toml`의
  `[ux] confirm_actions = "always" | "first_time" | "never"`로
  조정합니다 (기본 `always`).

## 4. Source Label

현재 UI는 2글자 약어 대신 **long-form label**을 사용합니다.

- `[Official]`: provider 문서나 vendor default에 직접 기대는 값
- `[Qmonster]`: 프로젝트 규칙이나 canonical guidance
- `[Heur]`: parser/policy heuristic
- `[Estimate]`: Qmonster 추정값

## 5. Severity

현재 severity badge는 다음 다섯 단계입니다.

- `SAFE`
- `GOOD`
- `CONCERN`
- `WARNING`
- `RISK`

Alert 제목과 pane recommendation 줄에서 같은 단어가 사용됩니다.

## 6. Provider Profile 표시

provider profile recommendation이 뜨면 pane 상세에 아래 형식으로 나옵니다.

```text
profile: claude-default (3 levers) [Qmonster]
[Official] KEY = VALUE — citation
side_effects (N):
- operator-visible trade-off
```

- profile 이름은 프로젝트가 정하므로 `[Qmonster]`
- 각 lever는 자기 source label을 따로 가집니다.
- aggressive profile만 `side_effects`가 붙고, baseline profile은 보통
  생략됩니다.
- Review-role profile인 `codex-review`와 `gemini-policy-review`도 같은
  형식으로 표시됩니다. 이 둘은 local-only / policy-review 운영 trade-off를
  보여주기 위해 `side_effects`를 함께 표시합니다.

## 7. 조작

- `Mouse wheel`: 포인터 아래 리스트나 modal 스크롤
- `Mouse left`: alert, pane, target 선택
- `Mouse double`: alert hide 토글
- `Mouse drag`: Alerts/Panes divider 드래그로 두 창 높이 조절
- `[` / `]`: Alerts 창 높이 줄이기 / 키우기 (Panes는 남은 높이 사용)
- `/`: Alerts/Panes split 비율 한 단계씩 순환
- `=`: Alerts/Panes split 기본값으로 reset
- `Enter/Space`: 선택된 alert hide 토글
- `Tab`: alerts / panes focus 전환
- `↑/↓`, `j/k`: 현재 focus된 리스트 한 칸 이동
- `PgUp/PgDn`: 페이지 단위 이동
- `Home/End`: 처음/끝으로 이동
- `t`: target picker 열기. 진입 키를 다시 누르면 overlay가 닫힙니다.
- `Enter`: session 선택 후 window 단계로 이동, 또는 window 확정
- `Left/Backspace`: window 단계에서 session 단계로 복귀
- `?`: help/legend overlay
- `r`: version drift 재확인
- `s`: snapshot 저장
- `u`: Claude pane에서는 statusline 기반 즉시 poll만 요청하고 provider 입력을
  보내지 않습니다. Codex/Gemini pane에서는 provider runtime slash source를
  하나씩 순환 실행해 상태 갱신을 요청합니다. `observe_only`에서는 Codex/Gemini
  pane 입력을 바꾸지 않기 위해 차단하고 `RuntimeRefreshBlocked`를 기록합니다.
  성공/실패는 `RuntimeRefreshRequested`, `RuntimeRefreshCompleted`,
  `RuntimeRefreshFailed`로 audit log에 남습니다.
- `m`: Metrics overlay 토글. ↑/↓ 또는 j/k로 비교 테이블의 pane을 선택하면
  아래 시계열 영역이 그 pane을 따라갑니다. m을 다시 누르거나 Esc / q /
  [x] 클릭으로 닫습니다.
- `y`: Alerts focus에서 선택된 alert의 `run` command를 system clipboard에
  복사합니다. 선택 항목에 `suggested_command`가 없거나 clipboard backend를
  열 수 없으면 `SystemNotice`로 이유를 표시합니다.
- `c`: system notice clear
- `p` / `d` / `y`: 기본은 사전 explainer 모달이 떠서 무엇을 send/reject/copy
  하는지, 왜 추천하는지(reason + source), audit chain, 그리고 현재
  actuation mode가 차단 중일 때(observe_only 등)의 경고를 보여줍니다.
  Enter로 확정, Esc / 같은 키 / [x] 클릭으로 취소. `[ux] confirm_actions`로
  빈도를 조정합니다 (always / first_time / never).
- `p` (확정 후 audit chain — Phase 5 safer-actuation): 선택된 pane의 pending
  prompt-send proposal 수락. audit chain은 actuation mode에 따라 달라짐:
  - Execute (`allow_auto_prompt_send=true`, 비 observe_only) → `PromptSendAccepted → PromptSendCompleted` 또는 `PromptSendFailed`
  - AutoSendOff (`allow_auto_prompt_send=false`, 비 observe_only) → `PromptSendAccepted + PromptSendBlocked` (2 이벤트)
  - observe_only → `PromptSendBlocked` 단독 (`PromptSendAccepted` 없음)
- `d` (확정 후 audit): 선택된 pane의 pending prompt-send proposal 기각
  (audit: `PromptSendRejected`; 모든 actuation mode에서 가용)
- `S`, `P`, `t`: 진입 키를 다시 누르면 overlay가 닫힙니다 (Settings는 숫자 편집
  중에는 예외 — 편집 모드에서는 S가 닫지 않음). 기존 q / Esc / [x] 닫기는
  그대로 유지됩니다.
- `S`: settings overlay 열기.
  화살표로 필드 이동, `e` 또는 `Enter`로 편집 시작, 숫자 입력 후
  `Enter`로 commit, `Esc`로 편집 취소, provider override row에서 `c`로
  override 제거, `w`로 loaded TOML에 저장합니다. `--config` 없이 시작해도
  표준 저장 경로는 `~/.qmonster/config/qmonster.toml`입니다.
- `q`, `Esc`: 종료 또는 overlay 닫기

## 8. Overlay

- **Choose Session / Choose Window**:
  왼쪽은 session -> window 트리, 오른쪽은 pane preview입니다.
- **Help**:
  스크롤 가능하며 `label : description` 정렬로 표시됩니다.
- **Git**:
  footer 오른쪽 아래 버전 배지를 클릭하면 열립니다.
  현재 repo root, branch, HEAD, upstream ahead/behind, worktree 변경 요약,
  최근 커밋을 보여줍니다.
- **Settings**:
  `S`로 열립니다. `1` / `2` / `3` / `4` / `5`,
  `Tab` / `Shift+Tab`, 또는 마우스 클릭으로
  `Thresholds` / `Integrations` / `Parameters` / `Rules` / `Badges`
  탭을 전환합니다. `Thresholds`는 cost / context / quota의
  warning/critical 값을 조정하고, `Integrations`는
  `[provider_setup] claude_sidefile` 및 `codex_app_server`를
  `Space` / `e` / `Enter` 또는 마우스 클릭으로 토글합니다.
  `Parameters`는 현재 주요 설정값과 기본값 차이를 보여주며,
  `Rules`는 cache / quota / reset / memory / security 정책이 발동하는
  조건을 읽기 전용으로 보여줍니다. `Badges`는 `CTX`, `COST`,
  `TOKENS`, `CACHE`, `RESET`, `CALLS`, `token io`, `cache io`와
  `[Official]` / `[Estimate]` / `[Heur]` / `[Qmonster]` source label의
  뜻을 설명합니다. modal 오른쪽 위 `[x]`를 클릭하거나
  `q` / `Esc`로 닫습니다. `w` 저장은 로드된 TOML의 코멘트와
  관련 없는 섹션을 보존하면서 Settings가 소유한 key만 갱신합니다.
- **Provider Setup (G-1, v1.29.0)**:
  `P`로 열립니다. 4개 탭(Claude / Codex / Gemini / Tmux)을 `1` / `2` / `3` / `4`로
  전환하며, provider 탭은 해당 statusline / footer / config 파일을
  Qmonster가 데이터를 수집할 수 있도록 어떻게 셋업할지 안내하고,
  Tmux 탭은 추천 4-pane 실행 환경을 설치하는 방법을 안내합니다.
  탭 본문은 두 부분으로 구성됩니다: (1) 현재 상태 헤더 — read-only
  filesystem 프로브로 감지한 `~/.claude/statusline.sh`,
  `~/.codex/config.toml`, `~/.gemini/settings.json`의 존재 여부와
  핵심 필드(예: `cache_read_input_tokens` export, `ui.footer.*`
  boolean) 상태; (2) 권장 설정 스니펫 — 복붙 가능한 텍스트로 렌더됩니다.
  - **Claude 탭**: cache 비율 계산이 포함된 추천 `statusline.sh` (bash).
    sidefile JSON export 블록
    (`~/.local/share/ai-cli-status/claude/<session_id>.json`) 포함 여부는
    `S` Settings → `Integrations`의
    `[provider_setup] claude_sidefile` 값으로 결정됩니다. Provider Setup
    안에서는 현재 값을 보여만 주며, 수정 위치가 Settings임을 안내합니다.
  - **Codex 탭**: `/statusline` 토글 리스트(어떤 항목이 bottom status에
    실리도록 권장하는지)와 `/status` welcome panel을 주기적으로 띄워
    `(+ N cached)` 필드가 Qmonster F-4 cache parser에 도달하게 하는
    가이드입니다. Codex App Server 사용 여부는 `S` Settings →
    `Integrations`의 `[provider_setup] codex_app_server` 값으로
    결정됩니다. Provider Setup은 현재 값을 표시하고, `y`로 복사되는
    안내 스니펫을 그 값에 맞춰 보여줍니다.
  - **Gemini 탭**: `~/.gemini/settings.json`의 `ui.footer.*` 권장 JSON
    템플릿과, OAuth는 그대로 유지하되 cache 필드는 FAQ-documented OAuth
    한계로 인해 노출되지 않는다는 informational note (API key 전환은
    운영자 선호에 따라 deferred).
  - **Tmux 탭**: 추천 4-pane 워크플로우 설치 스크립트를 `y`로 복사합니다.
    복사한 스크립트를 실행하면 `~/ts.sh` (Claude/Codex/Gemini/Qmonster
    pane을 만들고 `claude:1:main`, `codex:1:review`, `gemini:1:research`,
    `qmonster:1:monitor` title을 설정하는 launcher)와
    `~/.tmux/qmonster.tmux.conf` (mouse/history/navigation/title helper)를
    생성합니다. 실행 후 `tmux source-file ~/.tmux/qmonster.tmux.conf`,
    `~/ts.sh qmonster ~/Qmonster` 순서로 사용합니다.
  - **조작**: `1` / `2` / `3` / `4`, `Tab`, `←` / `→` 탭 전환,
    `↑` / `↓` 또는 `j` / `k` / mouse wheel 스크롤, `y` 현재
    탭 스니펫 복사, `q` / `Esc` 닫기.
  - **Read-only**: Qmonster는 어떤 provider 설정 파일에도 절대 쓰지
    않습니다. 운영자가 표시된 스니펫을 수동으로 복사해 적용합니다.
  - **v1.30.0 업데이트 (G-2)**: `qmonster.toml`에 새로
    `[provider_setup]` 섹션이 생겼습니다 (`claude_sidefile = true`
    기본 / `codex_app_server = false` 기본). 이 값들은 `S`
    Settings → `Integrations`에서 편집합니다. `P`로 overlay를 열면
    Provider Setup은 이 값을 읽어 현재 상태와 `y` 복사 대상을
    표시합니다. Sidefile-on-default는
    추천 `statusline.sh`가 라이브 세션 JSON을
    `~/.local/share/ai-cli-status/claude/<session_id>.json`에 그대로
    적어두게 하므로, 다운스트림 도구(F-5 reader 등)가 raw cache /
    cost / transcript_path를 그대로 읽을 수 있습니다.
  - **v1.30.0 업데이트 (F-5) — Claude CACHE 배지**: 추천
    `~/.claude/statusline.sh`가 CTX와 5h 사이에 옵션 `cache N%`
    토큰을 출력하면 Claude adapter가 이를 파싱해 `CACHE <N.N>%
[Official]` 배지를 Claude pane card에 표시합니다 (Codex CACHE
    배지와 동일한 UX). 이 값은 `SignalSet.cache_hit_ratio` 필드
    (0..1)로 들어가며, F-7 / F-7b cache rule (cache hot warning,
    cold compact, drift compact)이 Claude pane에도 발동합니다.
    배지가 안 보이면 `P` overlay → Claude 탭에서 cache % 계산이
    포함된 statusline.sh를 적용했는지 확인하세요.
  - **v1.31.0 업데이트 (F-5b) — Claude sidefile reader**: G-2
    sidefile-on-default가 떨군 세션 JSON 파일
    (`~/.local/share/ai-cli-status/claude/<session_id>.json`)을
    Qmonster가 직접 읽어 Claude pane card에 추가 정보를 채웁니다.
    JSON의 `cwd` 필드가 pane의 `current_path`와 같은 파일 중 mtime이
    가장 최근인 것을 선택하며, missing dir / malformed JSON /
    매칭 없음은 silent None으로 처리합니다 (Provider gate가
    Claude pane에만 적용 — Codex / Gemini가 같은 cwd를 공유해도
    Claude 세션 상태를 물려받지 않습니다). 결과:
    - **`cost $N.NN` 행**이 Claude pane card에 표시됩니다 (sidefile
      `cost.total_cost_usd`에서). 이전 statusline 전용 경로에서는
      Claude의 cost는 항상 None이었습니다.
    - **`5h resets in <eta>` / `7d resets in <eta>` 행**이 표시됩니다
      (sidefile `resets_at` 타임스탬프 기반). Claude tmux statusline은
      reset eta를 노출하지 않고 percentage만 보여주므로 sidefile만
      가능한 새 정보입니다. 포맷은 `2h13m` / `45m` / `30s`이며,
      14일 상한으로 sentinel 값을 거부합니다.
    - **`SID` / `XSCRIPT` runtime fact 배지**가 Claude pane card에
      표시됩니다 (sidefile `session_id` / `transcript_path`에서).
    - **`cache_hit_ratio` 정밀도 향상**: sidefile의 raw counts
      (`cache_read_input_tokens` / `input_tokens` /
      `cache_creation_input_tokens`)에서 정밀 비율을 계산해
      반올림된 statusline `cache N%` 값을 덮어씁니다. 이 결과
      F-7 / F-7b cache rule이 Claude pane에서 더 정밀하게
      발동합니다.
  - **v1.32.0 업데이트 (F-6) — Codex App Server**: G-2의
    `[provider_setup] codex_app_server = true` 토글이 켜져 있을 때
    Qmonster TUI는 startup에서 `codex app-server` 자식 프로세스를
    한 번 띄워 JSON-RPC `account/rateLimits/read`를 polling tick마다
    호출합니다. 응답의 5h / weekly 창이 갖는 `resets_at_unix_seconds`
    타임스탬프가 모든 Codex pane에 broadcast되어, Claude pane과
    동일한 **`5h resets in <eta>` / `7d resets in <eta>` 행**이
    Codex pane card에도 표시됩니다 (Codex tmux statusline은 reset
    timestamp를 노출하지 않고 percentage만 보여주므로 app-server만
    가능한 새 정보). 포맷은 F-5b의 `format_resets_eta`와 같은
    `2h13m` / `45m` / `30s`입니다. Pressure 필드 (`quota_5h_pressure`
    / `quota_weekly_pressure`)는 statusline 경로가 채우지 않았을
    때만 app-server 값으로 채워집니다 (`is_none()` 가드 — 기존
    per-pane 권한 유지). 시작 시 spawn 결과는 `SystemNotice`로
    안내됩니다 — 성공: "Codex App Server started" (Severity::Good),
    실패: "Codex App Server failed to start: <reason>"
    (Severity::Warning). Spawn 실패해도 TUI는 정상 시작합니다.
    reset eta가 안 보이면 `S` Settings → `Integrations`에서
    `codex_app_server`가 ON인지 확인하고 `w`로 저장한 뒤 Qmonster를
    재시작하세요. 별도 터미널에서 서버를 띄우거나 JSON-RPC 메시지를
    수동으로 보낼 필요는 없습니다. Qmonster가 startup에서
    `codex app-server`를 spawn하고 `initialize` 및
    `account/rateLimits/read`를 전송합니다. Linux에서는 bubblewrap 우회를 위해 spawn 시
    `-c sandbox_mode="danger-full-access"` 플래그가 자동으로
    추가됩니다.
  - **v1.33.0 업데이트 (F-4b) — Gemini /stats + /model parser**: 운영자가
    `u` 키를 cycle해 Gemini pane에서 `/stats session`, `/stats model`,
    그리고 idle/stale/limit-hit 상태일 때만 `/model`을 dispatch하면,
    그 출력을 Qmonster가 파싱해 다음 정보를 Gemini pane card에 채웁니다:
    - **누적 input / output token 카운트**: `/stats model` 출력의
      `Tokens` 섹션에서 `Total` / `Input` / `Output` 행을 읽어
      `input_tokens` / `output_tokens` 필드에 채웁니다 (`is_none()`
      가드 — 더 이른 surface가 이미 채웠다면 그대로 둠). model 파서는
      `input_tokens`와 `output_tokens`가 **둘 다** 추출됐을 때만 값을
      씁니다 (정직성 규칙: 반쪽 데이터 금지).
    - **CACHE 배지** (API key / Vertex Gemini만): `/stats model` 출력의
      `Cache Reads` 행이 보이면 `cached_input_tokens`로 들어가 CACHE
      배지가 표시됩니다. **OAuth Gemini는 FAQ-documented Google 제약**
      으로 인해 `Cache Reads` 행이 출력에 없으므로 CACHE 배지도
      나타나지 않습니다 — Qmonster는 0을 합성하지 않습니다 (정직성
      규칙).
    - **SID runtime fact 배지**: `/stats session` 출력의 `Session ID:`
      를 읽어 `RuntimeFactKind::SessionId` (F-5b에서 도입)가 채워지고,
      Gemini pane card에도 SID 배지가 표시됩니다.
    - **CALLS runtime fact 배지**: `/stats session` 출력의 `Tool Calls:`
      선두 숫자를 읽어 `CALLS <N> [Official]`로 표시합니다.
    - **RESET runtime fact / metric 배지**: `/model` 화면의 모델별
      `Reset:` / `Resets:` 행을 읽어 provider가 렌더한 reset 시각과
      남은 시간을 그대로 `RESET <model> <time/remaining> [Official]`
      형태로 표시합니다. Qmonster는 현재 status table의 `/model` 값을
      기준으로 같은 모델 family만 남깁니다 (`gemini-3.1-pro-preview`는
      `Pro`, `gemini-*-flash-lite`는 `Flash Lite`, `gemini-*-flash`는
      `Flash`). 전체 값은 `limits:` runtime fact 줄에 남고, 같은 값은
      상단 `metrics:` 줄에도 표시됩니다. 닫힌 `/model` picker 캡처는
      다음 `/model` 캡처나 pane lifecycle reset 전까지 유지됩니다. 이
      값은 Gemini 모델별 display-only runtime fact이며, F-7c
      reset-aware 정책 룰의 `quota_*_resets_at` 입력과는 별개입니다.
  - **v1.33.x polish (RESET 5H / RESET 7D 배지)**: 컴팩트 한-줄 metric
    행에 `RESET 5H <eta>` / `RESET 7D <eta>` 배지가 추가되어, F-5b의
    verbose `metric_row` 텍스트 행과 같은 `format_resets_eta` helper
    및 SourceKind 라벨링을 사용합니다. Claude sidefile 또는 Codex
    app-server 경로가 `quota_*_resets_at`을 채웠을 때 표시됩니다.
  - **v1.34.0 업데이트 (F-7c) — reset-aware 어드바이저리**: quota
    window가 reset에 가까워질 때 두 가지 신규 어드바이저리가 alert
    queue에 표시됩니다. 이는 기존의 `5h resets in <eta>` /
    `7d resets in <eta>` 배지(F-5b/F-6 경로)를 운영 행동으로
    연결합니다.
    - **`quota: pause until 5h/weekly window resets`** (Concern,
      ProjectCanonical): `quota_*_pressure >= 85%`이고 해당 window의
      reset이 30분 이내일 때 발화합니다. reason에는 실제 percentage,
      threshold(85%), 그리고 `2h13m` / `45m` / `30s` countdown이
      포함되어 RESET 5H/7D 배지와 일치합니다. next_step은 reset 시점
      까지 prompt 제출을 중단하라고 안내합니다.
    - **`snapshot before 5h/weekly window resets`** (Good,
      ProjectCanonical): 어떤 quota window라도 reset이 5분 이내이고
      해당 pressure가 50% 이상일 때 발화합니다. next_step은 `s` 키로
      runtime snapshot을 쓰도록 안내합니다.
    - 두 rule 모두 `IdentityConfidence >= Medium`을 요구하며
      `InputWait` / `PermissionWait` 상태에서는 suppress 됩니다.
      `quota_*_resets_at`이 채워지지 않은 pane(현재 Gemini)에서는
      발화하지 않습니다 (정직성 규칙).
  - **v1.35.0 업데이트 (F-7d) — operator-tunable `[reset]` 임계값**:
    F-7c의 4개 hardcoded 임계값이 `qmonster.toml`의 신규 `[reset]`
    섹션으로 노출됩니다. 운영자는 다음 키를 편집해 reset-aware
    어드바이저리를 자기 워크플로에 맞게 조정할 수 있습니다.
    기본값은 v1.34.0 (F-7c) hardcoded 값과 정확히 일치하므로 별도
    편집이 없으면 동작 변화가 없습니다.
    - `wait_pressure_threshold = 0.85` —
      `quota: pause until ... resets` 어드바이저리가 발화하는
      pressure 하한값
    - `wait_eta_secs = 1800` (30분) — 같은 어드바이저리가 요구하는
      reset까지 남은 시간 상한값
    - `snapshot_pressure_threshold = 0.50` —
      `snapshot before ... resets` 어드바이저리가 발화하는 pressure
      하한값 (운영자가 막 시작했다면 handoff 상태를 보존할 의미가
      없음)
    - `snapshot_eta_secs = 300` (5분) — 같은 어드바이저리가 요구하는
      reset까지 남은 시간 상한값
      `wait_for_reset` reason 문자열은 `gates.reset_wait_pressure`를
      그대로 보간하므로 `wait_pressure_threshold = 0.75`로 조정하면
      어드바이저리가 75% pressure에서 발화하고 reason에도 75%로
      표시됩니다 (원본 85%가 아님). `S` Settings → `Parameters`는
      현재 `[reset]` 값과 기본값 차이를 보여주고, `S` Settings → `Rules`
      는 `wait_for_reset` / `snapshot_before_reset` 발화 조건을 보여줍니다.
      편집은 아직 `qmonster.toml` 직접 수정 방식입니다. 다음 config 로드 시
      Qmonster가 새 값을 읽어옵니다.

### 8.5 Metrics Overlay

`m`으로 열리는 per-pane card overlay. 모든 pane의 메트릭을 한 화면에
인라인으로 보여주므로 ↑/↓로 pane을 따로 고를 필요가 없습니다.

각 카드:

- divider 한 줄 (`━ provider:instance:role · pane_id ━━…━`)
- 4개 content 행 — 두 열로 분리 (`<left> │ <right>`)

**왼쪽 열 (bounded)**: CTX / 5H / 7D / CACHE.

- bar 길이는 modal 폭에 따라 8–24셀로 자동.
- filled `█` cell: CTX/5H/7D는 severity 색 (SAFE 녹 / CONCERN 노 /
  WARNING 주 / RISK 빨 — `theme::severity_color`), CACHE는 중립 흰색.
- unfilled `░` cell: dim 회색 (`theme::TEXT_DIM`).
- 누락값은 dim `─`.

**오른쪽 열 (timeseries · counters)**: 4개 행으로 압축 —

- 행 1: `5H reset ▸ <eta>  ·  7D reset ▸ <eta>` (텍스트 only, progress
  bar 없음). 24h 이상은 `4d 6h` 형식, 미만은 `2h13m` / `45m` / `30s`.
  둘 다 누락이면 dim `─`. 한 쪽만 누락이면 그 segment + 분리자 함께
  drop.
- 행 2/3: `TOKENS in/out  <sparkline>  <current>  Δ+<delta>`. sparkline은
  modal 폭에 맞춰 동적 (~20셀+).
- 행 4: `COST <sparkline> $<v> <trend> · MEM <v> MiB <arrow> · MEM-FILE <bytes> <arrow>`.
  trend·arrow는 실측 ▲/▼/─ (per-poll MemObservation 트래커에서 산출
  — 첫 관측은 ─, 다음 폴부터 변화 반영).

상단 1줄에 `Hottest: <pane> · <metric> {pct}% [<source>]` 배너.
Bounded pressure 데이터가 전혀 없으면 dim `Hottest: —`.

조작:

- `m`, `Esc`, `q`, `[x]` 클릭: 닫기.
- `↑` / `↓` / `j` / `k`: body scroll (페인 선택 개념 없음 — 모든 pane이
  항상 보임).
- `[`: modal 5% 축소 (50%까지). `]`: 5% 확대 (99%까지). `=`: 기본
  95×90으로 reset. 폭·높이 동시 적용. 운영자가 고른 크기는 close 후
  재오픈에도 유지.
- 마우스휠: body가 modal 본문 높이를 넘으면 scroll.

누락된 값은 `─` 한 글자로 표시합니다 (S3-4 honesty 규칙).

### 8.6 Action Explainer

`p` / `d` / `y` 누름 시 사전 모달이 뜹니다. 표시되는 항목:

- **Target pane**: `provider:instance:role · pane_id`
- **What to send / What to reject / What to copy**: 실제로 보낼/거절할/복사할 텍스트
- **Why now**: 추천 이유 + source label (`[Official]` / `[Qmonster]` / `[Heur]` / `[Estimate]`)
- **Severity**: 해당 시 SAFE/GOOD/CONCERN/WARNING/RISK
- **Audit chain**: 현재 actuation mode에 따라 발화될 audit event 시퀀스
  (`Execute → PromptSendAccepted → PromptSendCompleted` 등)
- **Mode now**: actuation mode가 차단 중일 때 (`observe_only`, AutoSendOff)
  ⚠ 경고

조작:

- `Enter` 확정 → 기존 audit chain을 따라 동작 실행
- `Esc` / 같은 키(`p`/`d`/`y`) / `[x]` 클릭 → 취소 (실행하지 않음)

빈도 조정 (`~/.qmonster/config/qmonster.toml`):

```toml
[ux]
# always       — 매번 모달 (기본, 가장 안전)
# first_time   — 키 종류별로 한 번만, 그 이후는 즉시 실행
# never        — v1.37.0 즉시 실행 동작 복원
confirm_actions = "always"
```

`first_time`은 세션 로컬입니다 — Qmonster를 재시작하면 다시 모달이
뜹니다. 영구 silence를 원하면 `never`로 설정하세요.

### 8.7 Pending Actions Overlay

`a` 키로 토글합니다. v1.39에서 추가된 수면 가능한 디스커버리 layer로,
다음 항목을 한 번에 보여줍니다:

- pending prompt-send proposal을 보유한 모든 pane (operator가 `p`/`d`로
  처리할 수 있는 항목)
- `suggested_command`가 있는 모든 alert (operator가 `y`로 복사할 수
  있는 항목)

본 overlay 외에도 항목 존재는 두 가지 다른 surface로 노출됩니다:

- **Header chip** — pane card 제목에 `★p`, alert 제목에 `★y`가 붙고
  severity 색으로 강조됩니다.
- **Footer counter** — 화면 하단에 `★p:N · ★y:M` 카운터가 항상
  표시됩니다 (0이면 dim, 양수면 severity 색).

Overlay 행 형식: `<cursor> [p|y] <severity> · <command> · <context>`

조작:

- `↑` / `↓` 또는 `j` / `k` — 항목 이동
- `Enter` — 해당 pane/alert로 selection을 점프시키고 Action Explainer
  modal을 자동으로 엽니다 (실제 dispatch는 Action Explainer에서
  Enter로 확정)
- `a` 다시, `Esc`, `q`, `[x]` 클릭 — overlay 닫기

항목이 없을 때는 `No pending actions.` 안내 줄이 표시됩니다.

## 9. 운영 파일

- 표준 runtime root는 `~/.qmonster/`입니다.
- 표준 config path는 `~/.qmonster/config/qmonster.toml`입니다.
  `scripts/run-qmonster.sh`는 없으면 `config/qmonster.example.toml`에서
  복사하고, Qmonster를 항상 `--config`와 함께 실행합니다.
- 기본 tmux source는 `auto`입니다. startup 때 control-mode attach를 먼저
  시도하고 실패하면 polling으로 내려가며 startup notice를 남깁니다.
- forced control-mode smoke는 `scripts/run-qmonster-control-mode-once.sh`로
  수행합니다. 이 helper는 임시 config에만 `source = "control_mode"`를
  쓰고 `--once`로 종료하므로 표준 config를 수정하지 않습니다.
  helper가 `--config`/`--once`를 소유하므로 passthrough 인자는
  `--root`/`--set`만 허용합니다.
  `--once` 시작 출력의 `tmux source: control_mode` 줄로 실제 transport
  선택을 확인할 수 있습니다.
- 표준 pricing path는 `~/.qmonster/config/pricing.toml`입니다.
  없으면 `config/pricing.example.toml`이 복사됩니다. provider 가격은
  자주 바뀌므로 Qmonster가 자동 조회하지 않습니다. 운영자가 non-zero
  rate를 직접 채우면 Codex 등 cost_usd가 있는 pane에서 COST badge와
  cost_pressure advisory가 활성화됩니다.
