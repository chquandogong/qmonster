# Qmonster 구현 평가 및 개선 계획 (Codex)

작성일: 2026-05-08
범위: 최초 기획 문서(`.docs/init`의 최초 토큰 최적화 조사/구현 보고서), 현재 Rust 구현, `cargo test --all-targets`, `--once` 스모크, 라이브 tmux pane, Claude/Gemini 산출물 교차 확인.

## 평가 결론

Qmonster의 화면은 의미 있는 정보를 제공한다. pane별 provider, 역할, path, command, blocking state, context/quota/reset/token/cost/cache/memory, 추천, 이상 징후가 한 화면에 연결되고, Metrics overlay는 압력 bar와 token/cost/memory sparkline으로 단순하지만 운영 판단에 필요한 추세를 보여준다. `Token Insights`도 추천 상황, cache/cost 요약, action ledger, accepted/completed/ignored rate를 묶어 "추천이 실제로 쓰였는가"를 확인할 수 있다.

하지만 현재 구현은 "수집된 데이터가 맞는 pane/provider에 붙었는가"가 가장 큰 품질 리스크다. 라이브 스모크에서 `node /usr/bin/gemini --yolo` pane이 한때 `Claude`로 분류되어 Claude sidefile의 공식 token/cost/cache/reset 값이 붙었고, 현재 라이브 pane에서도 `cmd: qmonster`인 monitor pane이 canonical title 때문에 `Gemini research`로 표시된다. 즉 수치 자체는 공식 출처일 수 있어도 귀속 대상이 틀리면 화면의 신뢰도가 무너진다.

토큰 최적화는 방향이 맞고 이미 도움이 된다. cache hot/cold/drift 기반 `/compact` 추천, provider profile 추천, context pressure와 checkpoint-before-compact 정책은 기획의 "추천 우선, 자동 파괴 금지" 방향과 일치한다. 다만 아직 "얼마나 절약했는가"를 측정하지 않는다. 현재는 좋은 휴리스틱과 출처 라벨링의 단계이며, ROI/전후 비교/오탐 추적까지 닫혀야 최적화 효과를 증명할 수 있다.

이상 징후와 인사이트는 유용하지만 보정이 필요하다. 8종 detector와 recommendation 승격 구조는 의미가 있으나, cost slope는 `window_polls * 5초` 고정 가정으로 계산되고 실제 기본 poll interval과 어긋날 수 있다. token/cache/cost insight도 pane 단위가 아니라 창 전체 순서로 섞이면 그럴듯하지만 잘못된 성장치를 만들 수 있다.

## 근거

- 최초 기획은 observe-first, alert-first, token optimization architecture, provider별 공식/휴리스틱 출처 구분, raw archive와 preview 분리, checkpoint-before-compact를 요구했다.
- event loop는 tmux pane 수집, identity resolve, provider adapter parse, token/cost/anomaly/recommendation 저장과 effect delivery를 실제로 연결한다(`src/app/event_loop.rs:53`, `src/app/event_loop.rs:109`, `src/app/event_loop.rs:186`).
- token sample은 provider/pane별 누적값으로 SQLite에 저장되고 최근 sample을 pane별로 조회한다(`src/store/token_usage.rs:24`, `src/store/token_usage.rs:91`, `src/store/token_usage.rs:110`).
- pane UI와 metrics overlay는 source label, pressure trend, token/cost/memory sparkline을 실제로 렌더링한다(`src/ui/panels.rs:92`, `src/ui/metrics.rs:103`, `src/ui/metrics.rs:190`).
- cache rule은 cache ratio와 context pressure를 조합해 `/compact`를 기다리거나 실행할 타이밍을 추천한다(`src/policy/rules/cache.rs:7`, `src/policy/rules/cache.rs:85`, `src/policy/rules/cache.rs:133`, `src/policy/rules/cache.rs:179`).
- identity resolver는 canonical title을 최고 우선순위로 신뢰하고, command fallback은 `pane_current_command` 문자열만 본다(`src/domain/identity.rs:61`, `src/domain/identity.rs:75`, `src/domain/identity.rs:172`). `node /usr/bin/gemini` 같은 wrapper와 stale title 충돌에 취약하다.
- Claude sidefile 보강은 provider가 Claude이면 confidence나 command conflict를 추가로 확인하지 않고 cwd 기준 sidefile을 붙인다(`src/adapters/mod.rs:106`, `src/adapters/mod.rs:111`).
- insight cache/token growth는 전체 `token_usage_samples`를 시간순으로 훑어 first/latest input을 잡기 때문에 pane/provider가 섞일 수 있다(`src/store/insights.rs:502`, `src/store/insights.rs:558`).
- anomaly cost slope는 5초 polling interval을 코드에 고정한다(`src/policy/rules/anomaly.rs:552`, `src/policy/rules/anomaly.rs:576`).
- `cargo test --all-targets`는 통과했다. 기능 경로의 단위/통합 검증은 넓지만, 라이브 pane 식별 충돌 회귀 테스트는 부족하다.

## 개선 계획

### P0. Provider identity와 metric 귀속을 먼저 잠근다

목표: 화면의 모든 공식 수치가 올바른 provider/pane에만 귀속되게 한다.

- identity resolve 전에 descendant argv/exe 정보를 확보해 `node`, `bash` wrapper 뒤의 실제 CLI(`codex`, `gemini`, `claude`, `qmonster`)를 식별한다.
- canonical title과 실제 command/provider가 충돌하면 `High`로 승격하지 않고 `identity conflict` 상태로 표시한다.
- conflict 또는 `Low/Unknown` confidence에서는 provider-specific adapter enrichment, provider profile 추천, 공식 metric badge를 억제한다.
- Claude sidefile 보강은 cwd만으로 매칭하지 않고 session id, transcript path, descendant process, mtime TTL 중 최소 2개 이상의 근거가 맞을 때만 적용한다.
- ambiguous sidefile이면 `cache ?`, `cost ?`, `reset ?`로 표시하고 "suppressed: ambiguous Claude sidefile" evidence를 audit에 남긴다.
- 회귀 테스트를 추가한다: `node /usr/bin/gemini --yolo` + 같은 cwd의 Claude sidefile, stale `gemini:1:research` title + `cmd=qmonster`, canonical title/command 충돌 케이스.

### P1. Insight 집계를 pane 단위로 재설계한다

목표: 단순 통계가 "맞는 질문"에 답하게 한다.

- `Token Insights`의 cache reuse, token growth, cost delta를 먼저 pane/provider/session 단위로 계산한 뒤 전체 합계와 top contributors를 만든다.
- 보고서 상단에 `top token growth`, `top cost delta`, `cache drift panes`, `data completeness`를 추가한다.
- `first_input/latest_input` 전역 비교를 제거하고, counter reset과 provider/session 변경을 별도 구간으로 나눈다.
- action ledger와 recommendation lifecycle을 pane별 전후 metric window와 연결한다.
- `n/a`, `?`, `suppressed`, `provider unsupported`를 구분해 "데이터가 없어서 모름"과 "구조적으로 제공 불가"를 분리한다.

### P1. 토큰 최적화 ROI 루프를 닫는다

목표: `/compact`와 profile 추천이 실제로 도움 됐는지 측정한다.

- recommendation event id를 기준으로 추천 전 5-10분, 수락/거절/무시 후 5-10분의 token growth, cache ratio, cost delta를 비교한다.
- cache hot 상태에서 `/compact`를 피한 경우와 cache cold/drift 상태에서 `/compact`한 경우를 별도 outcome family로 기록한다.
- "saved tokens"는 공식값이 아니라 estimated로 라벨링하고, 추정식과 confidence를 함께 표시한다.
- profile switch 추천은 적용 여부뿐 아니라 이후 token slope/cost slope 감소 여부를 추적한다.
- accepted rate가 높지만 metric 개선이 없는 rule, ignored rate가 높고 오탐이 많은 rule을 `rule tuning candidates`로 표시한다.

### P1. Anomaly detector를 실제 시간과 데이터 완전성 기준으로 보정한다

목표: 이상 징후가 과장되거나 누락되지 않게 한다.

- cost slope와 token slope는 poll 수가 아니라 sample timestamp 차이로 정규화한다.
- sparse sample window에서는 `sample_count`, `coverage_pct`, `elapsed_secs`를 evidence에 표시하고, coverage가 낮으면 confidence를 낮춘다.
- memory growth는 absolute MB만 보지 말고 baseline 대비 비율, process restart, provider별 정상 변동 폭을 함께 본다.
- error burst는 단순 pattern bool이 아니라 dominant kind, 최근 command, provider state와 함께 보여준다.
- SubagentSideEffect는 현재처럼 correlation임을 유지하되, co-occurring anomaly 종류와 시간 간격을 evidence에 포함한다.

### P2. 화면을 더 복잡하게 만들지 않고 해석력을 높인다

목표: 이미 있는 단순 UI를 유지하면서 "왜 이 숫자를 믿어도 되는가"를 더 잘 보여준다.

- pane card에 `identity confidence`, `metric provenance`, `suppressed/conflict` badge를 한 줄로 묶는다.
- Metrics overlay에는 전체 sparkline보다 우선순위가 높은 pane 3개를 먼저 보여주고, 나머지는 접을 수 있게 한다.
- Insights overlay에는 raw log deep dive 대신 "evidence rows"를 우선 제공한다. 원문 로그는 필요할 때만 열도록 한다.
- provider setup overlay와 연결해 `cost ? [pricing]`가 어떤 pricing key 누락 때문인지 바로 알려준다.

### P2. Validation 문서를 현실 검증 중심으로 보강한다

목표: 테스트가 통과해도 라이브 pane에서 틀릴 수 있는 지점을 정기적으로 잡는다.

- `docs/ai/VALIDATION.md`에 live smoke checklist를 추가한다: tmux pane identity matrix, wrapper command, stale title, sidefile ambiguity, pricing missing, cache unsupported provider.
- `--once` 출력 fixture를 추가해 source label과 suppressed metric이 기대대로 표시되는지 확인한다.
- 현재 발견한 식별 오염 사례를 regression scenario로 남긴다.
- canonical docs에는 안정화된 결론만 승격하고, 실험/교차검증 기록은 모델별 계획 문서에 남긴다.

## 교차 검증 반영

### Gemini 문서 확인

`docs/gemini/improvement_plan.md`의 초기 평가는 화면, 통계, 시각화, SQLite 수집, token optimization, anomaly 체계를 전반적으로 긍정 평가했다. 방향성은 동의하지만, "데이터 수집 및 표출 정확성"은 너무 낙관적이었다. 실제 pane/`--once` 확인 결과 provider identity와 sidefile metric 귀속 문제가 있으므로, Codex 계획에서는 P0를 데이터 귀속 안정화로 조정했다.

Gemini는 이후 Claude/Codex 문서를 교차 검증하고 최종 섹션을 추가했다. 최종 Gemini 계획은 Codex의 `Data Attribution & Identity Stability`를 P0로 수용했고, Claude의 `Now Strip`, next-best-action, ROI payoff, anomaly evidence/ETA 제안을 P1/P2로 통합했다. Gemini의 기존 `Dynamic Layout Optimization`과 `Global Token Duplicate Detector`는 P3 장기 과제로 남았다. 이 최종 우선순위는 Codex 계획과 대체로 일치한다.

### Claude 최신 문서 확인

Claude 최신 문서는 `.docs/claude/Qmonster-v2.0.0-2026-05-08-claude-init-vs-impl-evaluation-r1.md`에 작성되었다. Claude의 결론은 "수집·저장·정직성은 spec을 초과 달성했지만, 표출은 분산되어 있고 인사이트는 reactive라서 causal/prescriptive synthesis가 부족하다"는 것이다.

Codex와 일치하는 지점:

- `Token Insights`는 데이터가 있지만 next-best-action과 payoff가 약하다.
- `/compact`나 profile 추천은 수락/거절 이후 실제 token/cost/cache 변화까지 닫혀야 "도움 됨"을 증명할 수 있다.
- anomaly는 detector와 저장은 충분하지만 evidence/narrative가 UI까지 풀리지 않는다.
- cost는 총합보다 pane/model/situation breakdown이 필요하다.
- 새 modal을 늘리기보다 현재 표면에 "지금 한 가지"와 evidence를 더 잘 합성해야 한다.

Codex가 더 강하게 보는 지점:

- Claude는 데이터 수집·표출을 `◎`로 평가했지만, Codex는 라이브 pane에서 provider identity conflict와 Claude sidefile metric 오염 가능성을 확인했다. 따라서 "수집은 충분"과 별개로 "올바른 pane/provider에 귀속되는가"를 P0로 둔다.
- Claude의 Slice A/B/C/E/D/H/G는 화면 합성력 개선에 집중한다. Codex 계획은 그 전에 identity authority와 metric suppression guard를 선행해야 한다고 본다.
- Claude는 `Now strip`을 1순위로 두었고, Codex는 `identity conflict`와 `sidefile ambiguity` 회귀 테스트를 1순위로 둔다. 잘못 귀속된 공식 수치를 더 잘 요약하면 오히려 더 위험하기 때문이다.

### 경로 및 대기 상태

Gemini 문서는 `docs/gemini/improvement_plan.md`에 있고, Codex 문서는 `docs/codex/improvement_plan.md`에 있다. Claude 원본은 `.docs/claude/Qmonster-v2.0.0-2026-05-08-claude-init-vs-impl-evaluation-r1.md`이며, 동일한 내용이 `docs/claude/Qmonster-v2.0.0-2026-05-08-claude-init-vs-impl-evaluation-r1.md`에도 확인된다. `docs/claude/improvement_plan.md`는 peer pane 대기 조건을 해소하기 위한 원본 위치 라우팅 문서다. 세 모델 모두 문서 작성과 교차 검증 업데이트를 완료했다.
