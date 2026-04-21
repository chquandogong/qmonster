# Qmonster UI 사용자 매뉴얼

Qmonster의 터미널 화면(TUI)은 크게 **상단 알림(Alerts) 영역**과 **하단 개별 Pane 상태(Panels) 영역**으로 나뉩니다. 화면을 꽉 채우고 있는 괄호 속 알파벳과 짧은 단어들은 모두 특정한 상태나 근거를 나타냅니다.

## 1. 🪪 정보의 출처 (SourceKind 뱃지)

화면에 나타나는 수치나 추천 알림이 **"어디서 온 정보인지(신뢰도)"**를 나타내는 2글자 약어입니다. 보통 `[PO]`, `[HE]` 형태로 수치나 알림 옆에 붙어 있습니다.

- **`[PO]` (Provider Official)**: Claude, Codex, Gemini, tmux 등의 **공식 문서나 제조사**가 보장하는 확실한 정보입니다.
- **`[PC]` (Project Canonical)**: 이 프로젝트(`Qmonster`)의 설정 파일이나 아키텍처 규칙에 명시된 **공식 프로젝트 내부 규칙**입니다.
- **`[HE]` (Heuristic)**: 커뮤니티 경험칙이나 패턴 분석을 통해 얻은 **경험적 추론**입니다. (예: "이런 패턴은 보통 로그 폭주더라")
- **`[ES]` (Estimated)**: Qmonster가 자체적으로 **추정한 값**이나 기본 임계값입니다.

---

## 2. 🚨 알림의 심각도 (Severity 문자)

상단 Alert 창이나 추천(Recommendation) 맨 앞에 붙는 1글자 알파벳으로, 해당 알림이 얼마나 위험하거나 중요한지 나타냅니다.

- **`[S]` (Safe)**: 안전함 (문제없음)
- **`[G]` (Good)**: 좋음 (권장되는 상태)
- **`[C]` (Concern)**: 우려됨 (주의를 기울일 필요가 있음, 예: 출력이 너무 길어질 조짐)
- **`[W]` (Warning)**: 경고 (조치가 필요함, 예: 로그 폭주, 사용자 입력 대기 중)
- **`[R]` (Risk)**: 위험 (즉각적인 확인 필요, 예: 파괴적인 명령어 권한 요청 대기)

---

## 3. 🏷️ 신원 추론 신뢰도 (Identity Confidence)

하단 개별 Pane의 제목(Title) 끝에 붙는 괄호 속 1글자 알파벳입니다. Qmonster가 해당 창이 "어떤 AI(Claude, Codex 등)인지" 알아낸 확신도입니다.
_예시: `%1 Claude:1:Main [H]`_

- **`[H]` (High)**: 매우 확실함
- **`[M]` (Medium)**: 중간 정도의 확신
- **`[L]` (Low)**: 불확실함 (단순 추측)
- **`[?]` (Unknown)**: 전혀 알 수 없음 (이 경우 특정 AI 전용 추천 알림이 제한됩니다)

---

## 4. 📟 상태 신호 칩 (Signal Chips)

해당 Pane에서 현재 어떤 이벤트가 감지되었는지 보여주는 짧은 텍스트 태그입니다. 여러 개가 동시에 뜰 수 있습니다. `signals: WAIT STORM` 처럼 표시됩니다.

- **`WAIT`**: 사용자 입력을 기다리는 중 (프롬프트 대기)
- **`PERM`**: 사용자의 권한 승인을 기다리는 중 (y/N 등)
- **`STORM`**: 로그 폭주 (텍스트가 너무 많이 쏟아지고 있음)
- **`REPEAT`**: 반복 출력 (이전에 본 것과 똑같은 결과가 계속 출력됨)
- **`VERB`**: 장황한 답변 (필요 이상으로 설명이 김)
- **`ERR`**: 에러 감지 (에러나 Stack Trace 형태의 텍스트가 감지됨)
- **`SUBAG`**: 서브에이전트 감지 (AI가 백그라운드에서 다른 에이전트를 몰래 실행함)
- **`—`**: 현재 감지된 특이 신호 없음

---

## 5. 📊 메트릭 (Metrics)

현재 AI가 사용 중인 자원과 비용을 나타냅니다. 메트릭 옆에는 항상 `[PO]`, `[ES]` 같은 출처 뱃지가 함께 표시됩니다.

- **`CTX=...%`**: Context Pressure (현재 대화 컨텍스트 창이 얼마나 찼는지 비율)
  - _예: `CTX=71% [ES]` (자체 추정 결과 컨텍스트가 71% 찼음)_
- **`TOKENS=...`**: 현재까지 사용된 토큰의 수
- **`COST=$...`**: 현재까지 소모된 비용 (달러 단위)

---

## 6. 🎚️ Provider Profile 추천 (Phase 4)

`Main` pane이 특정 provider(Claude/Codex/Gemini)로 건강하게 돌고 있을 때, Qmonster는 해당 provider의 CLI 레버(환경변수·설정키·플래그) 묶음을 추천 로우 아래에 **구조화된 profile 블록**으로 함께 표시합니다. 추천 헤더 `[G] [PC] apply profile ...` 다음 줄부터 시작합니다.

### 출력 형식

```
profile: <profile-name> (<N> levers) [PC]
[PO] <key> = <value> — <citation>
[PC] <key> = <value> — <citation>
...
side_effects (<M>):
- <operator-visible trade-off #1>
- <operator-visible trade-off #2>
```

- **헤더 로우 `profile: ... [PC]`**: profile 이름은 Qmonster가 정하므로 항상 `[PC]` (ProjectCanonical).
- **레버 로우 `[PO]`/`[PC]`**: 각 레버는 자체 SourceKind 뱃지를 가집니다. `[PO]`는 공식 문서가 키와 값을 모두 보장, `[PC]`는 키는 공식이거나 Qmonster 내부 개념이지만 VALUE 선택은 Qmonster의 판단.
- **side_effects 섹션**: aggressive profile은 operator 가시적 트레이드오프(예: "auto-memory 미사용 → CURRENT_STATE.md에 상태 기록 필수") 리스트를 레버 수만큼 함께 표시합니다 (Gemini G-6). baseline profile은 대부분 side_effects가 비어있어 섹션 자체가 생략됩니다.

### 6개 profile (3×2 grid)

| Provider × Mode | baseline (기본)  | aggressive (`quota_tight` opt-in 시) |
| --------------- | ---------------- | ------------------------------------ |
| **Claude**      | `claude-default` | `claude-script-low-token`            |
| **Codex**       | `codex-default`  | `codex-script-low-token`             |
| **Gemini**      | `gemini-default` | `gemini-script-low-token`            |

한 pane에서는 **정확히 0개 또는 1개**의 profile 추천만 발생합니다. baseline과 aggressive는 동일 pane에서 동시에 뜨지 않습니다 (`quota_tight` 게이트로 mutual exclusion). provider 게이트도 유지되어, Gemini pane은 `claude-*` / `codex-*` profile을 절대 받지 않습니다.

### aggressive 전환

Operator가 `quota_tight` 모드를 켜면(예: 긴 헤드리스 스크립트 세션) baseline이 억제되고 aggressive profile이 발화합니다. 공격적인 token-saving 레버(예: `--yolo = enabled`, `experimental.autoMemory = false`, `model_auto_compact_token_limit` 축소)가 제시되며, 각각의 트레이드오프가 `side_effects`로 즉시 가시화됩니다.
