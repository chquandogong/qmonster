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
  v1.51.0부터 비ASCII 알파벳 키(한글/카타카나 등)가 입력되면 divider가
  `⚠ HANGUL/IME ACTIVE — press 영문/English key to disable ⚠` 경고
  배너로 바뀌고 첫 활성화 시 터미널 BEL이 한 번 울립니다. ASCII
  알파벳을 입력하거나 3초간 비ASCII 알파벳이 없으면 자동으로 평소
  배너로 돌아갑니다. 터미널은 OS-level IME 상태를 앱에 노출하지
  않으므로 첫 비ASCII 키스트로크가 트리거 시점입니다.
- **Footer**: 2줄 상태 바입니다. 첫 줄은 현재 focus, Alerts/Panes split
  비율, `★p`/`★y`/`★a` 카운터를 보여주고, 둘째 줄은 왼쪽 `keys` 칩과
  자주 쓰는 핵심 키, 오른쪽 버전 배지를 한 줄에 맞춰 보여줍니다.
  `keys` 칩에 마우스를 올리거나 `K`를 누르면 기존의 긴 키 목록이
  title/footer 없는 넓은 key legend로 열립니다. legend는 Move / Layout /
  Actions / Overlays 그룹으로 나뉘어 한눈에 스캔할 수 있게 표시됩니다.
- **Overlay**: `t`로 target picker, `S`로 settings, `P`로 provider setup,
  `?`로 help, footer 오른쪽 아래 버전 배지를 클릭하면 Git overlay가
  열립니다. (narrow vNext에서 Metrics `m` / Anomaly Events `n` /
  Pending Actions `a` / Token Insights `i` / decorative fx `Q` 오버레이는
  제거되었습니다. 현재 상시 오버레이는 `?` Help과 `S` Settings, 그리고
  optional Git뿐입니다.)
- **Scroll status**: 주요 스크롤 가능한 modal/overlay와 list-style 창은
  footer/hint에 `scroll x/y · more` 또는 `scroll x/y · END`를 표시해
  더 내려갈 내용이 있는지 바로 알 수 있게 합니다.
- **Title/Footer role**: 큰 스크롤형 overlay는 title에는 창 정체성만 두고,
  footer/hint에는 조작법과 scroll status를 둡니다. 짧은 confirmation modal과
  hover help는 예외입니다.
- **Hover Help**: Alerts/Panes 행 위에 마우스를 올리면 floating help가
  뜹니다. 내용이 길면 줄바꿈을 반영해 높이가 커지고, 터미널이 좁거나
  카드가 잘릴 수 있으면 화면 하단 drawer 형태로 열립니다. `H`로 on/off,
  `L`로 한국어/영어를 전환합니다.

### Overlay chrome contract

Large persistent overlays share the same chrome controls:
the same entry key closes the overlay, and `[x]` / `Esc` / `q` close
where the overlay is not in an edit sub-mode. `S` Settings keeps
edit-mode guards on number editing; its read-only tabs scroll with
`↑` / `↓` / `j` / `k` / mouse wheel / `PgUp` / `PgDn` / `Home` / `End`.
Short confirmation modals intentionally remain non-resizable. For the
large scrollable overlays, the title identifies the window and the
footer/hint carries controls plus `scroll x/y · more/END`.

(narrow vNext removed the movable/resizable `m` / `a` / `i` / `n`
overlays, so the `[` / `]` resize · `=` geometry-reset · title-row drag
chrome no longer applies to any overlay — those keys now act only on
the Alerts/Panes split.)

### Floating hover help

기본값은 켜짐(`ko`)이며, `label` trigger는 마우스가 행 앞쪽 라벨 영역에
있을 때만 help를 엽니다. `row` trigger로 바꾸면 기존처럼 행 전체 hover에서
열립니다. Alerts에서는 `bulk hide`, 헤더, `dismiss`,
`summary`, detail(`next`/`run`/`anchor`/`others`), copy hint를 행별로
설명합니다. 최상단 `Now` row는 현재 우선순위 요약을 설명합니다.
Panes에서는 헤더(provider/role/CLI version), `state`,
`path`, `cmd`, `status`, `signals`, metrics, tokens/cache io, runtime
facts(`session`/`loaded` 포함), recommendations를 행별로 설명합니다.
하단 상태 줄의 `★p`, `★y`, `★a`는 각각 prompt-send 제안 수, 복사 가능한
alert 수, 최근 audit 심각도에 대한 help를 엽니다 (display-only 카운터 —
클릭해도 오버레이를 열지 않습니다). footer의 `keys` 칩은
`ux.hover_help = false`여도 항상 key legend를 열 수 있는 예외입니다.
hover help 제목에는 현재 언어와 `H/L` 힌트가 표시되고, 본문 하단에는
`H` toggle, `L` language, `S` Settings 저장 위치가 같이 표시됩니다.
명시적 overlay가 열려 있으면 dashboard hover help는 숨겨집니다.

런타임 단축키:

- `H`: floating hover help on/off
- `L`: help language `ko`/`en` 전환

영구 설정은 `S` Settings → Parameters에서 `ux hover_help`,
`ux help_language`, `ux hover_help_trigger`를 수정한 뒤 `w`로 저장하거나
TOML에 직접 적습니다.
`S` Settings → Parameters 안에서도 `H` / `L`을 누르면 같은 설정을 즉시
바꿀 수 있고, 변경은 `w` 저장 전까지 runtime-only 상태입니다.

```toml
[ux]
hover_help = true
help_language = "ko" # "ko" | "en"
hover_help_trigger = "label" # "label" | "row"
```

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

### Alert Flow 읽는 법

- `FLOW` 행은 서로 관련된 recommendation alert들이 하나의 대응 흐름을
  만들 때 표시됩니다. 첫 구현 범위는 context/cache/snapshot/`/compact`
  계열의 **Context recovery** 흐름입니다.
- 기본 목록에서는 원본 alert들이 중복 top-level 행으로 흩어지지 않고,
  `FLOW Context recovery · N alerts · active` 한 행으로 대표됩니다.
- `summary` 아래 rail은 `o` / `|` 접두사로 원인 신호, 후속 근거,
  선행 조치, 실행 명령을 순서대로 보여줍니다.
- 선택된 FLOW 행은 `included` 행으로 묶인 원본 alert action과 source를
  보여줍니다. 이는 flow가 어떤 근거로 만들어졌는지 확인하기 위한
  evidence입니다.
- FLOW에 실행 가능한 command가 있으면 기존 alert와 같이 제목에 `★y`가
  붙고, Alerts focus에서 `y`로 현재 대표 command를 복사합니다. Context
  recovery에서는 `/compact`가 있으면 그것을 우선 복사합니다.
- Enter/Space hide는 FLOW key를 대상으로 합니다. 같은 구성의 flow가
  숨겨진 동안 included 원본 alert들은 top-level로 다시 흩어지지 않습니다.
- `related` / `rail` 행은 FLOW로 접을 만큼 강한 관계는 아니지만 같은 pane의
  command 또는 recovery signal을 공유하는 alert들이 있을 때 표시됩니다.
  이 경우 alert들은 계속 개별 top-level 항목으로 남고, hide/copy/severity
  정렬도 각 alert 기준으로 유지됩니다. 선택되지 않은 alert는 짧은
  `related` 요약만 보여주며, 선택된 alert만 `rail`과 `included` 행으로
  관련 sibling action/source를 펼쳐 보여줍니다.
- Alerts는 Panes처럼 카드화하지 않고 row queue를 유지합니다. 선택된
  alert/FLOW도 `summary`, `run`, `related`, FLOW timeline 같은 기본 행은
  선택 전과 같은 형태로 유지하고, 그 아래에만 `├` / `└` / `│` tree glyph로
  `rail`, `included`, `action` 보조 detail을 덧붙입니다. 따라서 커서를
  움직여도 기존 행의 의미가 바뀌지 않고, 선택된 row에서만 추가 근거와
  copy action을 확인할 수 있습니다.

- Alerts 맨 위 `bulk hide :` 줄의 severity chip은 **actionable alert만**
  대상으로 합니다. `c`로 지울 수 있는 system notice는 여기에 포함되지
  않습니다.

## 3. Panes 읽는 법

- pane 제목은 현재 다음 형태입니다. CLI 버전이 확인되면 provider role과
  pane id 사이에 버전 배지가 붙습니다.

```text
session:window · Provider role · CLI <version> [Official] · %pane_id
session:window · Provider role · %pane_id
```

- 예:
  `qmonster:0 · Codex review · CLI 0.122.0 [Official] · %57`
- 선택된 pane가 펼쳐진 상태에서는 같은 정보를 한 줄 목록으로 섞지 않고
  **4개 구역** `IDENTITY`, `NOW`, `PRESSURE`, `NEXT`로 나눠 보여줍니다
  (narrow vNext에서 재구성 — 기존 NOW/WHERE/PRESSURE/RUNTIME/
  RECOMMENDATIONS 5구역을 단순화). `IDENTITY`는 안정적인 "누가/어디서"
  정보로 path/cmd 행을 담습니다 (provider/role/CLI 버전과 IDENTITY
  CONFLICT 줄은 카드 제목 줄에 그대로 남습니다). `NOW`는 현재
  state/blocked/signals/proposal, `PRESSURE`는 metrics/tokens/cache에
  더해 이전 `RUNTIME` 구역의 provider runtime facts(modes/access/loaded
  /restrict)를 합쳐 담습니다, `NEXT`는 추천과 그 detail을 담습니다
  (이전 `RECOMMENDATIONS`에서 이름 변경). 접힌 pane 행은 기존처럼 flat
  row로 유지됩니다.
- 펼친 pane card는 각 섹션 헤더 아래 행에 트리 글리프를 덧붙입니다.
  형제 행 중 마지막은 `└ `, 그 외에는 `├ `로 시작하고, 랩 발생 시의
  continuation 라인은 위쪽 형제가 더 있으면 `│ `, 마지막 형제의 본문
  이면 공백 2칸으로 이어집니다. 깊이당 2 cell씩 들여쓰기되며, `NEXT`의
  하위 디테일(`next` / `run` / `lever` / `effect`)은 한 단계 더 들어
  갑니다. 트리 분기는 섹션 폭을 깎지 않도록 `section_wrap = wrap_width
  - 2`, `detail_wrap = wrap_width - 4`로 wrap budget을 미리 차감합니다.
- `path` 행은 pane의 cwd가 linked git worktree(`git worktree add` 형
  sibling checkout)이면 ` · wt of <parent-repo-root>` 접미사를 붙여
  부모 repo root를 함께 보여줍니다 (v2.3.0). 결과는 `WorktreeRoleCache`
  (10초 TTL, 128-key LRU)로 캐싱되어 폴링당 git 호출이 폭주하지
  않습니다. Primary worktree와 non-git cwd는 변동 없이 기존 표시 그대로
  유지됩니다.
- `CLI` 버전 배지는 Qmonster monitor pane에는 표시하지 않습니다. provider가
  화면에 직접 노출한 버전을 우선 사용하고, 없으면 `/proc`에서 현재 pane의
  descendant CLI `pid`/`exe`/`argv`를 확인한 뒤 그 exact executable/script에
  `--version`을 실행해 얻은 값만 표시합니다. 현재 pane의 실행 버전이라고
  확정할 수 없으면 배지 자체를 생략합니다.
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
  `log storm`, `verbose output`, `error hint`, `subagent activity`
  (`repeated output` 칩은 v2.2.0에서 dead-code 정리로 제거 — 어떤
  adapter도 채우지 않던 신호였습니다.)
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
  (bottom-status `1.51M in / 20.4K out` and `/status` token usage) and
  Claude sidefile (`input_tokens` / `output_tokens` / cache reads).
  (Gemini는 narrow vNext에서 status-table-core로 축소되어 더 이상
  `/stats model` 토큰 enrichment를 제공하지 않습니다.)
- `CACHE` badge shows the cache hit ratio for the pane's cumulative
  prompt input — `cache_hit_ratio = cached_input_tokens /
(input_tokens + cached_input_tokens) × 100`, formatted with one
  decimal. Source label tracks `cached_input_tokens.source_kind`
  (`[Official]` for Codex `/status` and Claude sidefile/statusline
  cache). Format:
  `cache <N.N>%` (text) or `CACHE <N.N>%` (TUI). `CACHE ?` means the
  provider can expose cache data but this tick has not produced it yet.
  `CACHE —` means the current provider/auth surface has produced related
  stats while omitting Cache Reads, so cache reuse is structurally
  unavailable for that pane. When selected-pane details have raw cache
  counts, Qmonster also shows `cache io: read <N> / create <N>`;
  `create` is currently Claude sidefile `cache_creation_input_tokens`
  and is not folded into the hit-ratio badge.
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
  Codex는 선택된 pane에서 `u`를 누르면 provider의 read-only runtime
  slash command와 terminal submit(`C-m`, Enter-equivalent)을 보냅니다:
  Codex `/status`.
  narrow vNext (Slice 6c)에서 Gemini의 `/model` + `/stats session` +
  `/stats model` interactive enrichment는 제거되었습니다 — Gemini는
  status-table-core로 축소되어, 항상 보이는 status table(context /
  quota / memory / model)이 core 신호를 이미 담고 있으므로 `u`를 눌러도
  더 이상 slash command를 보내지 않습니다. Claude pane에서 Qmonster가
  읽는 capture-ready 표면(`/context` · `/usage` · `/status` · `/stats`)은
  운영자가 직접 열어둔 화면을 파싱하는 surface일 뿐, `u`가 Claude에
  명령을 보내지는 않습니다.
  `thinking...` 진행 표시가 있으면 tail이 몇 poll 동안 같아도 `IDLE`로
  떨어지지 않고, live prompt가 남아 있어도 최근 tail이 변하는 동안은
  active로 유지됩니다. 다음 poll에서 캡처와 읽을 수 있는 로컬 provider
  설정을 `RuntimeFact`로 파싱합니다.
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
- 선택된 pane는 recommendation을 `NEXT` 구역 아래로 펼쳐서 보여줍니다.
  (narrow vNext에서 provider-profile recommender가 제거되어, lever 목록을
  나열하던 `profile:` payload 블록은 더 이상 표시되지 않습니다.)
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

## 6. Provider Profile 표시 (제거됨 — narrow vNext)

provider-profile recommender(3×2 grid: Claude/Codex/Gemini × baseline/
aggressive, review-tier `codex-review` / `gemini-policy-review`, lever +
`side_effects` 렌더)는 narrow vNext에서 통째로 제거되었습니다 —
`Recommendation.profile` 필드와 `ui::panels::format_profile_lines`
렌더러도 함께 삭제되었습니다. pane 상세에는 더 이상 `profile:` 블록이
표시되지 않습니다.

남아 있는 것: `[profile_switch]` opt-in 룰(error-rate 기반으로
script-low-token 프로파일 *이름*을 추천하는 별개 룰; lever payload를
렌더하지 않음)과 `[security] posture_advisories` 같은 일반 recommendation은
계속 `NEXT` 구역에 나타납니다.

## 7. 조작

- `Mouse wheel`: 포인터 아래 리스트나 modal 스크롤
- `Mouse left`: alert, pane, target 선택
- `Mouse double`: alert hide 토글
- `Mouse drag`: Alerts/Panes divider로 두 창 높이 조절
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
  보내지 않습니다. Codex pane에서는 provider runtime slash source(`/status`)를
  실행해 상태 갱신을 요청합니다. narrow vNext에서 Gemini는 status-table-core로
  축소되어 `u`가 어떤 slash command도 보내지 않습니다(즉시 poll만). `observe_only`
  에서는 Codex pane 입력을 바꾸지 않기 위해 차단하고 `RuntimeRefreshBlocked`를
  기록합니다. 성공/실패는 `RuntimeRefreshRequested`, `RuntimeRefreshCompleted`,
  `RuntimeRefreshFailed`로 audit log에 남습니다.
- `y`: Alerts focus에서 선택된 alert의 `run` command를 system clipboard에
  복사합니다. `run` command는 실제 shell command 또는 provider slash
  command만 대상입니다. `# ...` 주석이나 `<placeholder>`가 포함된 값은
  copy 대상에서 제외되고, 선택 항목에 복사 가능한 command가 없거나
  clipboard backend를 열 수 없으면 `SystemNotice`로 이유를 표시합니다.
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
- `S`: settings overlay 열기 (탭 `1` Thresholds / `2` Integrations /
  `3` Parameters — narrow vNext에서 `Rules` / `Badges` 탭은 제거).
  화살표로 필드 이동, `e` 또는 `Enter`로 편집 시작, 숫자 입력 후
  `Enter`로 commit, `Esc`로 편집 취소, provider override row에서 `c`로
  override 제거, `w`로 loaded TOML에 저장합니다. `Parameters` read-only
  탭에서는 `↑` / `↓` / `j` / `k` / mouse wheel로 body scroll, `PgUp` /
  `PgDn` page scroll, `Home` / `End` 처음/끝 이동을 지원합니다.
  `--config` 없이 시작해도 표준 저장 경로는
  `~/.qmonster/config/qmonster.toml`입니다.
- `q`, `Esc`: 종료 또는 overlay 닫기

## 8. Overlay

- **Choose Session / Choose Window**:
  왼쪽은 session -> window 트리, 오른쪽은 pane preview입니다.
- **Help**:
  스크롤 가능하며 `label : description` 정렬로 표시됩니다. `Hover Help`
  섹션에는 Alerts/Panes hover 범위, 작은 터미널에서 bottom drawer로
  전환되는 조건, `S > Parameters`의 `Selected parameter help`, `H` / `L`
  키의 의미가 함께 정리되어 있습니다. hint 끝에는 현재 scroll 위치와
  `more` / `END` 상태가 표시됩니다.
- **Git**:
  footer 오른쪽 아래 버전 배지를 클릭하면 열립니다.
  현재 repo root, branch, HEAD, upstream ahead/behind, worktree 변경 요약,
  최근 커밋을 보여줍니다.
- **Settings**:
  `S`로 열립니다. **3개 탭** `Thresholds` / `Integrations` / `Parameters`를
  `1` / `2` / `3`, `Tab` / `Shift+Tab`, 또는 마우스 클릭으로 전환합니다
  (narrow vNext에서 `Rules` · `Badges` 탭은 제거되었습니다 — 둘 다
  제거된 overlay/badge 표면을 설명하던 read-only 탭이었습니다).
  `Thresholds`는 cost / context / quota의 warning/critical 값을 조정하고,
  `Integrations`는 `[provider_setup] claude_sidefile` 하나만 `Space` /
  `e` / `Enter` 또는 마우스 클릭으로 토글합니다 (codex_app_server
  토글은 app-server enrichment와 함께 제거).
  `Parameters`는 현재 주요 설정값과 기본값 차이를 보여줍니다. 충분히 넓은
  화면에서는 왼쪽이 설정 리스트, 오른쪽이 `Selected parameter help` 패널인
  2-column layout으로 표시됩니다. 좁은 화면에서는 같은 내용을 stacked
  layout으로 표시합니다. help 패널은 TOML key, 현재값과 기본값, 의미, 허용
  값, 관련 shortcut, `w` 저장 전까지 runtime-only라는 저장 상태를 설명합니다.
  여기에는 `[reset]` snapshot/wait threshold, `[cache]` 임계값,
  `[provider_setup]` 상태 등이 포함됩니다.
  Parameters 탭에서는 `H`가 `ux.hover_help`, `L`이 `ux.help_language`를
  즉시 토글하고 dirty 상태로 표시합니다. `ux.hover_help_trigger`는
  `label` 또는 `row`로 cycle/edit할 수 있습니다.
  `Parameters` read-only body가 modal 높이를 넘을 때 `↑` / `↓` / `j` /
  `k` / wheel / `PgUp` / `PgDn` / `Home` / `End`로 스크롤합니다. modal
  오른쪽 위 `[x]`를 클릭하거나 `S`를 다시 누르거나 (`숫자 편집 중 제외`)
  `q` / `Esc`로 닫습니다. `w` 저장은 로드된 TOML의 코멘트와 관련 없는
  섹션을 보존하면서 Settings가 소유한 key만 갱신합니다.
- **Provider Setup (G-1, v1.29.0; v2.4.0에서 `agy` 5번째 탭 추가)**:
  `P`로 열립니다. 5개 탭(Claude / Codex / Gemini / agy / Tmux)을
  `1` / `2` / `3` / `4` / `5`로 전환하며, provider 탭은 해당
  statusline / footer / config 파일을 Qmonster가 데이터를 수집할
  수 있도록 어떻게 셋업할지 안내하고, Tmux 탭은 추천 4-pane 실행
  환경을 설치하는 방법을 안내합니다.
  탭 본문은 두 부분으로 구성됩니다: (1) 현재 상태 헤더 — read-only
  filesystem 프로브로 감지한 `~/.claude/statusline.sh`,
  `~/.codex/config.toml`, `~/.gemini/settings.json`의 존재 여부와
  핵심 필드(예: `cache_read_input_tokens` export, `ui.footer.*`
  boolean) 상태; (2) 권장 설정 스니펫 — 복붙 가능한 텍스트로 렌더됩니다.
  Provider Setup에서 연결한 telemetry는 pane card의 metrics/cache/quota
  배지 품질을 높입니다. 관련 threshold는 `S` Settings에서 확인합니다.
  - **Claude 탭**: cache 비율 계산이 포함된 추천 `statusline.sh` (bash).
    sidefile JSON export 블록
    (`~/.local/share/ai-cli-status/claude/<session_id>.json`) 포함 여부는
    `S` Settings → `Integrations`의
    `[provider_setup] claude_sidefile` 값으로 결정됩니다. Provider Setup
    안에서는 현재 값을 보여만 주며, 수정 위치가 Settings임을 안내합니다.
  - **Codex 탭**: `/statusline` 토글 리스트(어떤 항목이 bottom status에
    실리도록 권장하는지)와 `/status` welcome panel을 주기적으로 띄워
    `(+ N cached)` 필드가 Qmonster F-4 cache parser에 도달하게 하는
    가이드입니다. (narrow vNext에서 Codex App Server enrichment와 그
    `[provider_setup] codex_app_server` 토글은 제거되어 Codex reset-ETA
    행은 더 이상 제공되지 않습니다.)
  - **Gemini 탭**: `~/.gemini/settings.json`의 `ui.footer.*` 권장 JSON
    템플릿과, OAuth는 그대로 유지하되 cache 필드는 FAQ-documented OAuth
    한계로 인해 노출되지 않는다는 informational note (API key 전환은
    운영자 선호에 따라 deferred).
  - **agy 탭 (v2.4.0; narrow vNext에서 ObserveOnly-only로 환원)**:
    Google이 2026-06-18부터 free / Pro / Ultra / Code Assist 개인
    라이선스에서 Gemini CLI를 대체하기로 발표한 새 Antigravity
    CLI(`agy`)를 위한 탭입니다. 짧고 솔직한 안내 — agy는 Antigravity
    IDE의 launcher이고 문서화된 headless API가 아직 없으므로,
    Qmonster는 `agy` 패널을 **ObserveOnly로 식별만** 합니다
    (`agy:N:role` canonical title 또는 pane current_command `agy`
    토큰). 모든 분석 표면(anomaly / profile-switch / cache / cost /
    token sample)은 독립 게이트로 차단됩니다.
    Enterprise / Cloud / Standard Gemini CLI 라이선스는 2026-06-18
    이후에도 유지되므로 Gemini 탭은 계속 유효합니다.

    (narrow vNext에서 agy enrichment — footer scrape + structured
    sidefile로 `model_name` · `context%` · `token_count` · quota를
    채우던 v2.7.0/v2.8.0 `agy_enrichment` 경로와 그 `statusLine.command`
    권장 스니펫 + `[provider_setup] agy_enrichment` 토글 — 은 통째로
    제거되었습니다. agy 탭은 다시 짧은 ObserveOnly 안내만 보여주며,
    어떤 enrichment 배지도 표시하지 않습니다.)

  - **Tmux 탭**: 추천 4-pane 워크플로우 설치 스크립트를 `y`로 복사합니다.
    복사한 스크립트를 실행하면 `~/ts.sh` (Claude/Codex/Gemini/Qmonster
    pane을 만들고 `claude:1:main`, `codex:1:review`, `agy:1:research`,
    `qmonster:1:monitor` title을 설정하는 launcher)와
    `~/.tmux/qmonster.tmux.conf` (mouse/history/navigation/title helper)를
    생성합니다. `~/ts.sh`는 새 session을 만들 때
    `~/.tmux/qmonster.tmux.conf`가 있으면 pane split 전에 자동으로
    source합니다. 현재 tmux server에 helper binding을 즉시 반영하려면
    `tmux source-file ~/.tmux/qmonster.tmux.conf`를 한 번 실행한 뒤
    `~/ts.sh qmonster ~/Qmonster`를 사용합니다.
  - **조작**: `1` / `2` / `3` / `4` / `5`, `Tab`, `←` / `→` 탭 전환
    (v2.4.0부터 `4 agy` / `5 Tmux`로 키 이동),
    `↑` / `↓` 또는 `j` / `k` / mouse wheel 스크롤, `y` 현재
    탭 스니펫 복사, `P` 다시 / `q` / `Esc` 닫기.
  - **Read-only**: Qmonster는 어떤 provider 설정 파일에도 절대 쓰지
    않습니다. 운영자가 표시된 스니펫을 수동으로 복사해 적용합니다.
  - **v1.30.0 업데이트 (G-2)**: `qmonster.toml`에 새로
    `[provider_setup]` 섹션이 생겼습니다 (`claude_sidefile = true`
    기본 — narrow vNext 기준 Integrations가 토글하는 유일한 키). 이 값은 `S`
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
  - **Codex App Server (F-6) — 제거됨 (narrow vNext)**: `codex
app-server` JSON-RPC `account/rateLimits/read` enrichment와 그
    `[provider_setup] codex_app_server` 토글이 제거되었습니다. Codex의
    `5h resets in <eta>` / `7d resets in <eta>` reset-ETA 행은 더 이상
    제공되지 않습니다 (Claude sidefile reset eta는 그대로 유지). Codex의
    `quota_5h_pressure` / `quota_weekly_pressure`는 계속 bottom status
    line에서 채워집니다.
  - **Gemini /stats + /model enrichment (F-4b) — 제거됨 (narrow vNext)**:
    `u` 사이클로 `/stats session` · `/stats model` · `/model`을
    dispatch해 누적 토큰 / Cache Reads / SID / CALLS / 모델별 RESET을
    채우던 interactive enrichment가 제거되었습니다. Gemini는 다시
    status-table-core(`context` / `quota` / `memory` / `model`)만
    파싱합니다. Gemini `u`는 어떤 slash command도 보내지 않습니다.
  - **v1.34.0 업데이트 (F-7c) — reset-aware 어드바이저리**: quota
    window가 reset에 가까워질 때 두 가지 신규 어드바이저리가 alert
    queue에 표시됩니다. 이는 Claude sidefile이 채우는 `5h resets in
<eta>` / `7d resets in <eta>` 배지(F-5b 경로)를 운영 행동으로
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
      현재 `[reset]` 값과 기본값 차이를 보여줍니다.
      편집은 아직 `qmonster.toml` 직접 수정 방식입니다. 다음 config 로드 시
      Qmonster가 새 값을 읽어옵니다.
  - **v1.42.0 업데이트 (Phase H) — opt-in auto-snapshot at reset boundary**:
    `[reset] auto_snapshot = true`로 설정하면 Qmonster는 F-7c
    `recommend_snapshot_before_reset` 어드바이저리가 발화할 때마다
    `(pane, quota window)`당 snapshot을 자동으로 한 번 기록합니다.
    snapshot은 `<qmonster-root>/snapshots/<timestamp>.json`에 저장되며,
    `SnapshotWritten` audit 이벤트에는 `trigger=auto_reset_boundary` 및
    `quota_kind=5h|weekly` 메타데이터가 summary 문자열로 기록됩니다.
    쓰기가 완료되면 Concern-severity `SystemNotice`가 한 번 표시되어
    operator가 경로를 확인할 수 있습니다. recommendation 자체는 대시보드
    recommendation 패널에 계속 표시됩니다 — Phase H는 이를 소비하거나
    숨기지 않습니다.

    기본값: `auto_snapshot = false`. `[reset]` 섹션이 없는 v1.41.0
    operator는 동작 변화가 없습니다.

    F-7d의 operator-tunable 임계값(`snapshot_pressure_threshold`,
    `snapshot_eta_secs`)이 recommendation 발화 조건을 제어합니다;
    Phase H는 별도의 임계값 없이 이 값들을 그대로 재사용합니다.

### 8.5 Action Explainer

`p` / `d` / `y` 누름 시 사전 모달이 뜹니다 (단일 prompt-send actuation의
confirmation gate — narrow vNext에서 BATCH `a` Pending Actions overlay는
제거되고 이 단일 액션 경로만 유지). 표시되는 항목:

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
hover_help = true
help_language = "ko" # "ko" | "en"
hover_help_trigger = "label" # "label" | "row"
```

`first_time`은 세션 로컬입니다 — Qmonster를 재시작하면 다시 모달이
뜹니다. 영구 silence를 원하면 `never`로 설정하세요.

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
