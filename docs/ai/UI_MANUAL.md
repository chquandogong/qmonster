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
  `System Notice`, `Checkpoint`, `Cross-Pane`, 일반 recommendation 제목
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
  `CTX 90%`, `QUOTA 47%`, `TOKENS 12345 [Official]`, `COST $0.42 [Estimate]`,
  `MODEL gpt-5.4 [Official]`
- `CTX` badge는 수치가 높을수록 더 강한 severity 색을 사용합니다.
  85% 이상은 `Risk`, 75% 이상은 `Warning`, 60% 이상은 `Concern`으로
  취급됩니다. `QUOTA` badge는 Gemini 전용으로 같은 severity 임계치를
  공유합니다 (Slice 3 S3-3).
- 현재 `CTX`는 구조적으로 확인 가능한 provider status에서만 채웁니다.
  Codex는 bottom status line, Gemini는 status table의 `context` 컬럼을
  사용합니다. Claude의 `/status` 사용량 막대는 context window가 아니라
  usage/rate limit이므로 `CTX`로 표시하지 않습니다.
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
  `tokens  : Main 1.51M in / 20.4K out [Official]` 형태의 breakdown을
  추가로 보여줍니다. Subagent token 분리는 아직 신뢰 가능한 provider
  signal이 없어 표시하지 않습니다.
- `MODEL` badge는 source가 있을 때만 표시합니다. Claude pane은
  `~/.claude/settings.json`에 `"model"` 키가 있을 때만 채워지므로,
  사용자 환경이 그 키를 비워둔 상태(=Claude Code가 기본 모델
  선택을 동적으로 하는 상태)에서는 의도적으로 빈칸으로 둡니다.
  허위 표시(예: `claude --version` 결과를 모델 이름으로 둔갑)
  대신 부재가 곧 honesty라는 전제입니다 (S3-4 design decision (b)).
  Codex / Gemini는 status surface에서 직접 `gpt-…` / `gemini-…`
  토큰을 읽을 수 있으면 채웁니다.
- 긴 worktree 경로 문자열은 PATH badge에서 40자까지 자동
  ellipsize됩니다 (Slice 3 housekeeping). 잘린 부분은 `…` 한 글자로
  표시되어 badge 한 줄이 pane card 폭을 넘기지 않습니다.
- `modes` / `access` / `loaded` / `restrict` 줄은 provider runtime fact를
  표시합니다. Qmonster는 선택된 pane에서 `u`를 누르면 provider의
  read-only runtime slash command와 terminal submit(`C-m`, Enter-equivalent)을
  보냅니다. 여러 runtime surface가 있는 provider는 `u`를 누를 때마다 하나씩
  순환 실행합니다: Claude `/status` → `/usage` → `/stats`, Codex
  `/status`, Gemini `/stats session` → `/stats model` → `/stats tools`.
  Claude `/status`는 실행 후 화면이 계속 남기 때문에 Qmonster가 먼저 그
  출력을 캡처해 one-shot parser overlay로 저장하고, 이어서 `Escape`를 보내
  pane이 다음 명령을 받을 수 있게 되돌립니다. Claude는 다음 순환 명령을
  보내기 전에도 방어적으로 `Escape`를 보내 이전 fullscreen runtime surface를
  닫습니다. Gemini는 pre-`Escape` 없이 stats 명령만 순환합니다. 다음 poll에서
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
- `t`: target picker 열기
- `Enter`: session 선택 후 window 단계로 이동, 또는 window 확정
- `Left/Backspace`: window 단계에서 session 단계로 복귀
- `?`: help/legend overlay
- `r`: version drift 재확인
- `s`: snapshot 저장
- `u`: 선택된 pane의 provider runtime slash source를 하나씩 순환 실행해 상태
  갱신 요청. `observe_only`에서는 pane 입력을 바꾸지 않기 위해 차단하고
  `RuntimeRefreshBlocked`를 기록합니다. 성공/실패는
  `RuntimeRefreshRequested`, `RuntimeRefreshCompleted`,
  `RuntimeRefreshFailed`로 audit log에 남습니다.
- `y`: Alerts focus에서 선택된 alert의 `run` command를 system clipboard에
  복사합니다. 선택 항목에 `suggested_command`가 없거나 clipboard backend를
  열 수 없으면 `SystemNotice`로 이유를 표시합니다.
- `c`: system notice clear
- `p`: 선택된 pane의 pending prompt-send proposal 수락 (Phase 5 safer-actuation). audit chain은 actuation mode에 따라 달라짐:
  - Execute (`allow_auto_prompt_send=true`, 비 observe_only) → `PromptSendAccepted → PromptSendCompleted` 또는 `PromptSendFailed`
  - AutoSendOff (`allow_auto_prompt_send=false`, 비 observe_only) → `PromptSendAccepted + PromptSendBlocked` (2 이벤트)
  - observe_only → `PromptSendBlocked` 단독 (`PromptSendAccepted` 없음)
- `d`: 선택된 pane의 pending prompt-send proposal 기각 (audit: `PromptSendRejected`; 모든 actuation mode에서 가용)
- `S`: cost / context / quota threshold settings overlay 열기.
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
  `S`로 열립니다. cost / context / quota의 default, claude, codex,
  gemini warning/critical threshold를 한 화면에서 조정합니다. modal
  오른쪽 위 `[x]`를 클릭하거나 `q` / `Esc`로 닫습니다. `w` 저장은
  `~/.qmonster/config/qmonster.toml` 또는 명시적 `--config PATH`에
  `toml::to_string_pretty` 형식으로 씁니다. 이 저장 방식은 현재
  comment-preserving이 아닙니다.

## 9. 운영 파일

- 표준 runtime root는 `~/.qmonster/`입니다.
- 표준 config path는 `~/.qmonster/config/qmonster.toml`입니다.
  `scripts/run-qmonster.sh`는 없으면 `config/qmonster.example.toml`에서
  복사하고, Qmonster를 항상 `--config`와 함께 실행합니다.
- control-mode trial은 `scripts/run-qmonster-control-mode-once.sh`로
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
