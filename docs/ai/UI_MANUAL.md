# Qmonster UI 사용자 매뉴얼

현재 Qmonster TUI는 **상단 Alerts**, **하단 Panes**, **footer**, 그리고
필요할 때 열리는 **overlay**들로 구성됩니다. 이 문서는 현재 구현된
표기와 조작만 설명합니다.

## 1. 화면 구성

- **Alerts**: 현재 target 기준의 경고/추천 큐입니다. 제목에는
  `visible`, `new`, `auto-hide` 개수가 표시됩니다.
- **Panes**: 선택된 session/window 안의 pane 목록입니다. 현재 선택된
  pane는 같은 리스트 안에서 상세 내용이 아래로 펼쳐집니다.
- **Footer**: 현재 focus와 주요 조작 키를 보여줍니다.
- **Overlay**: `t`로 target picker, `?`로 help, footer 오른쪽 아래
  버전 배지를 클릭하면 Git overlay가 열립니다.

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
  `state`, `path`, `status`, `blocked`, `signals`, `metrics`,
  `modes`, `access`, `loaded`, `restrict`
- `state` 줄은 pane가 멈춤/대기 상태일 때만 보입니다.
  상태 배지(`IDLE`, `WAIT`, `USAGE LIMIT`)와 경과 시간 배지(`⏱ MM:SS` 또는 `H:MM:SS`)가 함께 표시됩니다.
- `status`는 현재 `high confidence`, `medium confidence`,
  `low confidence`, `unknown confidence`처럼 텍스트로 표시됩니다.
- `blocked` 줄은 가장 중요한 대기 상태만 따로 보여줍니다.
  `waiting for input`, `approval needed`
- `signals` 줄은 그 외 상태를 보여줍니다.
  `log storm`, `repeated output`, `verbose output`, `error hint`,
  `subagent activity`
- `metrics` 줄은 badge 형태로 표시됩니다.
  `CTX 90%`, `TOKENS 12345 [Official]`, `COST $0.42 [Estimate]`
- `CTX` badge는 수치가 높을수록 더 강한 severity 색을 사용합니다.
  85% 이상은 `Risk`, 75% 이상은 `Warning`, 60% 이상은 `Concern`으로
  취급됩니다.
- 현재 `CTX`는 구조적으로 확인 가능한 provider status에서만 채웁니다.
  Codex는 bottom status line, Gemini는 status table의 `context` 컬럼을
  사용합니다. Claude의 `/status` 사용량 막대는 context window가 아니라
  usage/rate limit이므로 `CTX`로 표시하지 않습니다.
- `modes` / `access` / `loaded` / `restrict` 줄은 provider runtime fact를
  표시합니다. Qmonster는 선택된 pane에서 `u`를 누르면 provider의
  read-only runtime slash command를 보냅니다. Claude에는 `/status`,
  `/config`, `/stats`, `/usage`를 순서대로 보내고, Codex/Gemini에는
  `/status`와 실행 Enter를 보냅니다. 다음 poll에서 그 공식 출력과 읽을 수
  있는 로컬 provider 설정을 `RuntimeFact`로 파싱합니다.
  예: `PERM`, `MODE`, `SANDBOX`, `DIR`, `AGENTS`, `TOOL`, `SKILL`,
  `PLUGIN`.
- 이 줄들은 “보였다”가 아니라 “provider status/config source에서 확인된”
  값만 보여줍니다. 해당 provider가 특정 값(예: 전체 tool registry나
  active skill list)을 slash/status로 노출하지 않으면 Qmonster는 값을
  꾸며내지 않고 빈 줄로 둡니다.
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
- `u`: 선택된 pane에 provider runtime status/config slash command를 보내 상태
  갱신 요청. `observe_only`에서는 pane 입력을 바꾸지 않기 위해 차단하고
  `RuntimeRefreshBlocked`를 기록합니다. 성공/실패는
  `RuntimeRefreshRequested`, `RuntimeRefreshCompleted`,
  `RuntimeRefreshFailed`로 audit log에 남습니다.
- `c`: system notice clear
- `p`: 선택된 pane의 pending prompt-send proposal 수락 (Phase 5 safer-actuation). audit chain은 actuation mode에 따라 달라짐:
  - Execute (`allow_auto_prompt_send=true`, 비 observe_only) → `PromptSendAccepted → PromptSendCompleted` 또는 `PromptSendFailed`
  - AutoSendOff (`allow_auto_prompt_send=false`, 비 observe_only) → `PromptSendAccepted + PromptSendBlocked` (2 이벤트)
  - observe_only → `PromptSendBlocked` 단독 (`PromptSendAccepted` 없음)
- `d`: 선택된 pane의 pending prompt-send proposal 기각 (audit: `PromptSendRejected`; 모든 actuation mode에서 가용)
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
