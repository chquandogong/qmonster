---
title: Qmonster v2.0.0 — `.docs/init/` 원안 vs 실제 구현 평가 + 개선 계획 (r2, cross-validated)
author: Claude (Opus 4.7, 1M context)
date: 2026-05-08
version_at_evaluation: v2.0.0 (commit 5456934)
supersedes: docs/claude/Qmonster-v2.0.0-2026-05-08-claude-init-vs-impl-evaluation-r1.md
related_init_docs:
  - .docs/init/Qmonster_종합_기획_보고서_v0.4.0_2026-04-20.md
  - .docs/init/Qmonster_ms_init_prompt_ko_v0.4.0_2026-04-20.txt
  - .docs/init/Qmonster_토큰_최적화_조사_및_구현_보고서_2026-04-19 2.md
peer_plans_consumed:
  - docs/codex/improvement_plan.md
  - docs/gemini/improvement_plan.md
status: r2 — codex/gemini 교차 검증 반영
---

# 0. r2 갱신 요약

> **r1 → r2 변경의 한 줄.** Codex가 *라이브 tmux pane*에서 **identity/sidefile 오귀속 버그**(node wrapper 뒤의 진짜 CLI 미식별, stale title, cwd-only Claude sidefile match)를 잡았다. 이는 r1 전체가 _전제로 가정한_ "공식 수치는 올바른 pane에 붙는다"를 깬다. 따라서 r2는 **새 Slice 0 (Attribution Lock)을 모든 다른 슬라이스보다 먼저 둔다**. 또한 Codex P1(insight pane-bucketing) + 시간 정규화(anomaly cost-slope 5s 하드코딩) 핵심 지적과, Gemini의 cross-agent token duplicate / dynamic layout 방향성을 본 계획에 통합했다.

**합의 (3-way agreed)**

- 화면·메트릭 시각화는 단순하지만 의미 있다 (Claude ○ / Codex ○ / Gemini ○).
- SourceKind 라벨링·SQLite 영속·8 detector는 spec을 충실히 또는 초과 달성 (Claude ◎ / Codex ○ / Gemini ◎).
- 토큰 최적화 5층 구조는 정확하지만 **ROI loop가 닫혀 있지 않다** — `/compact` 수락 후 절감량을 운영자가 못 본다 (Claude G3 / Codex P1 ROI / Gemini "비용 반응형").

**상충 (cross-check 핵심)**

- **Gemini는 데이터 수집을 `매우 신뢰`로 평가**, 하지만 **Codex는 라이브 스모크에서 오귀속 사례 2건 확인**: (a) `node /usr/bin/gemini --yolo` 가 한때 Claude로 분류 → Claude sidefile의 공식 token/cost/cache가 Gemini pane에 붙었음, (b) `cmd: qmonster` monitor pane이 stale `gemini:1:research` title 때문에 Gemini로 표시. ⇒ **Codex의 P0가 우선**. r1은 이 위험을 보지 못했다.
- **Codex는 pane-level bucket이 부족한 insights 집계 결함**도 지적: `first_input/latest_input`을 전역 시간순으로 추적하면 pane/provider 섞여 잘못된 token growth 산출 가능. r1 Slice C(payoff)는 이 문제를 인식하지 못한 채 lifecycle store SELECT만 가정했음. r2에서 Slice C 알고리즘에 pane-bucket precondition 추가.
- **anomaly cost-slope/token-slope 시간 정규화**: Codex가 `window_polls * 5초` 하드코딩 발견. r1 Slice E(ETA)도 동일 문제 가능 — 회귀 방지를 위해 sample timestamp 차이로 정규화하도록 수정.

**합의된 최종 우선순위 (r2)**

1. Slice 0 — **Attribution Lock** (identity/sidefile/conflict suppression) — Codex P0
2. Slice 1 — **Pane-bucketed Insights re-aggregation** — Codex P1
3. Slice 2 — **ROI loop closure** (`/compact` payoff before/after) — Claude G3 / Codex P1 / Gemini 비용 반응형
4. Slice 3 — **Anomaly time-normalization + evidence enrichment** — Codex P1 + Claude G4
5. Slice 4 — **Now strip / Next-best-action** (front page synthesis) — Claude G1/G2
6. Slice 5 — **ETA chips with sample-timestamp normalization** — Claude G5 (Codex 시간 정규화 교훈 반영)
7. Slice 6 — **Live smoke validation enrichment** — Codex P2
8. Slice 7 — **Sandbox/audit visibility** — Claude G6
9. Slice 8 — **Cost breakdown** — Claude G7 / Gemini cross-agent dup
10. Slice 9 — **Dynamic layout / responsive auto-collapse** — Gemini

**보류 (이번 라운드 안 함 — 합의)**

- 새 modal 추가하지 않음 (3-way 합의).
- 자동 actuation 추가하지 않음 — Gemini "비용 반응형 자동 전환"은 _제안_ 수준에서 stop, 자동 실행은 spec §9.1 Layer 5 위반 (Claude/Codex 모두 동의).
- 새 detector 추가하지 않음 — 기존 8개의 attribution과 시간 정규화부터 안정화.

---

# 1. 평가 (r1과 동일하지만 cross-check 결과 보강)

## 1.1 5개 평가 축의 r2 점수

| 축                                  | r1 점수 (단독) | r2 점수 (교차 검증) | 점수 변동 사유                                                                                                                                   |
| ----------------------------------- | -------------- | ------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------ |
| (1) 화면이 의미 있는 정보를 주는가  | △              | △                   | 변동 없음 (Codex 동의, Gemini ○로 평가하지만 7 modal 분산은 제기 안 함)                                                                          |
| (2) 분석·통계·시각화가 의미 있는가  | ○              | ○                   | 변동 없음 (3-way 합의)                                                                                                                           |
| (3) 데이터가 제대로 수집·표출되는가 | ◎              | ○ → △               | **r1 ◎ 하향**: 수집 깊이는 ◎이나 *pane 귀속 정확성*에 Codex가 발견한 실제 버그 존재. 데이터 자체는 진짜이나 *어느 pane에 붙는가*가 깨질 수 있음. |
| (4) 토큰 최적화가 진짜 도움 되나    | △              | △                   | 3-way 합의 (ROI loop 부재)                                                                                                                       |
| (5) 이상 징후 인사이트가 도움 되나  | △              | △                   | 3-way 합의. cost-slope 시간 단위 버그 추가 발견 (Codex).                                                                                         |

**핵심 r2 통찰**: 축 (3)은 spec 정신상 “출처 honesty”이 1순위였는데 (provider-honesty 4-state, SourceKind 라벨), Codex 라이브 스모크는 라벨 자체가 정확해도 *그 라벨이 누구에게 붙는가*가 깨질 수 있음을 보여줬다. 이는 spec §11.4 “official 수치와 estimated 수치 구분”의 *전제*를 흔드는 발견이다. r1은 이 위험을 보지 못했다.

## 1.2 spec 일치 매트릭스 (r2 보정)

| spec 항목                       | r1 평가   | r2 평가   | 비고                                                                    |
| ------------------------------- | --------- | --------- | ----------------------------------------------------------------------- |
| §11.4 official vs estimated     | ◎         | ○         | 라벨 자체는 ◎, 라벨의 귀속 정확성에 Codex 발견 버그 1개                 |
| §13.2 자주 변하는 runtime state | ◎         | ○         | provider attribution이 깨질 수 있다는 점에서 강하게 ◎이라 부르긴 어려움 |
| 나머지 17개 항목                | r1과 동일 | r1과 동일 |                                                                         |

---

# 2. r1에는 없던 새 발견 (Codex/Gemini 기여)

## 2.1 Codex 기여 (라이브 스모크 결과)

### 2.1.1 Identity 오귀속 (P0)

> `src/domain/identity.rs` resolver가 canonical title을 최고 우선순위로 신뢰하고, command fallback은 `pane_current_command` 문자열만 본다. `node /usr/bin/gemini` 같은 wrapper와 stale title 충돌에 취약하다.

검증 후 sources:

- `src/domain/identity.rs:61` (canonical title resolver)
- `src/domain/identity.rs:75` (instance 추론)
- `src/domain/identity.rs:172` (command fallback)
- `src/adapters/mod.rs:106` + `:111` (Claude sidefile은 cwd-only 매칭)

운영 영향: **공식 수치(Claude statusline의 ctx %, cache %, 5h %, 7d %, cost)가 잘못된 pane에 붙으면 Insights 통계도, ETA 예측도, ROI 계산도 모두 오염된다**. 즉 r1의 모든 후속 슬라이스의 전제가 깨진다.

### 2.1.2 Insights 전역 집계의 pane 섞임 (P1)

> `src/store/insights.rs:502, :558` — `first_input/latest_input`을 전체 `token_usage_samples`를 시간순으로 훑어 잡는다. pane/provider가 섞일 수 있다.

운영 영향: 한 시간 동안 Codex pane에서 5K → 35K, Gemini pane에서 2K → 4K로 변했다면, 전역 first/latest는 (5K → 4K) 또는 (2K → 35K)처럼 pane을 가로질러 잘못 짝지어진다. r1 Slice C(payoff) 알고리즘이 이 결함 위에서 짜여 있었음.

### 2.1.3 Anomaly cost-slope 시간 단위 하드코딩 (P1)

> `src/policy/rules/anomaly.rs:552, :576` — cost slope가 `window_polls * 5초` 가정. 실제 기본 poll interval과 어긋날 수 있다.

운영 영향: poll interval을 변경하면 cost slope의 단위가 틀려져 false positive/negative가 발생.

### 2.1.4 라이브 smoke checklist 부재 (P2)

> 1445 lib + 65 integration test 통과해도 라이브 pane에서 식별 충돌·sidefile ambiguity·pricing missing은 회귀할 수 있다. `--once` fixture와 live identity matrix가 `docs/ai/VALIDATION.md`에 부재.

## 2.2 Gemini 기여 (장기 방향성)

- **Deep Dive view**: anomaly/metric 선택 시 원본 로그 chunk를 diff로 inline 표시. 이는 Claude r1 Slice D(anomaly evidence expansion)와 정렬되며, 표면 형태는 Gemini 안이 더 명확.
- **Cross-Agent token duplicate detector**: 같은 prompt가 여러 agent에 중복으로 들어가는 경우를 잡는 새 detector. 이는 spec §9.2 시나리오 외 새 차원이지만 cross-pane 토큰 절감에 의미 있음.
- **Cost-reactive profile switch**: 비용 slope 가파르면 저비용 프로필로 전환 _제안_. Gemini 안은 “자동 실행”까지 언급하지만, spec §9.1 Layer 5 “destructive 자동화 금지” 위반이므로 r2에서는 *제안 수준*만 채택.
- **Dynamic layout / auto-collapse**: 작은 viewport에서 중요도 낮은 정보 자동 접기. r1 G1(modal 분산) 문제 완화에 직접 기여.

---

# 3. 개선 계획 r2 (cross-validated)

## 3.0 우선순위 원칙 (r1 + 추가)

(r1 5개 원칙 유지)

추가:

6. **데이터 귀속을 깊은 분석보다 먼저 잠근다** (Codex). 잘못 귀속된 공식 수치는 미세 권고 백 개보다 운영자 신뢰를 더 빨리 무너뜨린다.
7. **시간 단위는 sample timestamp로 정규화한다** (Codex). poll count 기반 추론은 poll interval 변경 시 회귀.
8. **자동 actuation 라인은 절대 넘지 않는다** (3-way 합의). Gemini가 제기한 “비용 반응형 자동 전환”은 *제안*까지만.

## 3.1 Slice 0 — Attribution Lock (NEW, Codex P0)

> 모든 후속 슬라이스의 전제. 화면의 모든 공식 수치가 *올바른 pane*에 귀속되도록 잠근다.

**대상 코드**:

- `src/domain/identity.rs` — resolver 진입점.
- `src/adapters/mod.rs:106, :111` — Claude sidefile 보강.
- `src/adapters/process_memory.rs` — 이미 descendant tracking이 있음, identity에서 재사용 가능.

**작업**:

1. **Descendant argv/exe 탐지**: tmux pane의 PID에서 자식 프로세스 트리를 walk하여 `node`, `bash`, `python`, `sh` wrapper 뒤의 진짜 entry binary (`codex`, `gemini`, `claude`, `qmonster`)를 발견.
2. **Title vs command 충돌 감지**: canonical title의 provider와 descendant entry binary가 다르면 `IdentityConfidence::Unknown`이 아닌 새 상태 `IdentityConfidence::Conflict`로 표시 (또는 Low로 강등).
3. **Conflict 상태에서 enrichment 차단**: provider-specific adapter parse, provider profile 추천, 공식 metric badge 모두 억제. 카드 헤더에 `IDENTITY CONFLICT` chip + “provider says X, command says Y” 한 줄.
4. **Claude sidefile 다중 근거 매칭**: cwd만으로는 매칭하지 않고 (a) session id, (b) transcript path, (c) descendant process exe, (d) sidefile mtime TTL 중 **최소 2개** 일치. 모호하면 sidefile 보강 안 하고 audit `Suppressed: ambiguous Claude sidefile` 기록.
5. **회귀 테스트** (Codex 제안 그대로):
   - `node /usr/bin/gemini --yolo` + 같은 cwd의 stale Claude sidefile.
   - Stale `gemini:1:research` title + `cmd=qmonster`.
   - canonical title이 `claude`인데 descendant exe가 `codex`인 경우.
6. **`--once` fixture**: 위 3 시나리오를 `tests/integration/once_fixtures/` 아래 황금 파일로 넣고 source label/suppressed metric 표시를 lock.

**Done-when**:

- 새 `IdentityConfidence::Conflict` enum + 3 회귀 테스트 통과.
- Conflict pane에서 `cost ?`, `cache ?`, `reset ?`, `model ?` 표시 (Hidden이 아니라 Pending). r1 G6 sandbox/approval strip이 자연스럽게 이 4-state를 재사용.
- audit_events에 `IdentitySuppressed` kind 추가, retention 정책 동일 적용.
- 1445 lib + 65 integration test 그대로 통과.

**위험**: descendant process tree walk가 race condition. 회피: tick 안에서 한 번만 walk하고 lifetime으로 cache + identity가 흔들릴 때만 invalidate. /proc race는 신뢰도 confidence 1단계 다운으로 흡수.

**우선순위**: **최상위. 1번 슬라이스 머지 전 다른 슬라이스 시작 금지.**

## 3.2 Slice 1 — Pane-bucketed Insights re-aggregation (Codex P1)

> Insights overlay의 통계가 pane/provider 단위로 먼저 계산되도록 재설계.

**대상 코드**:

- `src/store/insights.rs:502, :558` — first/latest input 전역 추적 부분.
- `src/insights_report.rs` — 표출 포맷.

**작업**:

1. SQL을 `GROUP BY pane_id` 기반으로 재작성. 각 pane별 first/latest input, cost_delta, cache_ratio.
2. 보고서 상단에 추가 섹션:
   ```
   Top contributors
     token growth:  Codex review +27K · Claude main +12K · Gemini research +2K
     cost delta:    Codex review $0.74 · Claude main $0.38
     cache drift:   Codex review (3 events) · Claude main (1 event)
     data completeness: Claude 100% · Codex 95% · Gemini 50% (2 polls missing)
   ```
3. counter reset 처리: 한 pane이 새 session으로 재시작하면 token counter도 리셋되므로, *역방향 변화*는 “new session detected” 마크로 분리.
4. provider 변경(Slice 0의 IdentityConfidence::Conflict 검출과 연결)이 발생한 구간은 별도 구간으로.
5. 4-state 표시 정밀화: `n/a`(데이터 없음), `?`(pending), `suppressed`(conflict 원인), `unsupported`(provider 구조상 불가) 명확 분리.

**Done-when**:

- 기존 `format_insights_report_lines` 출력 형식 보존(backward-compat) + 새 “Top contributors” + “data completeness” 섹션 추가.
- pane-bucket fixture (3 pane × 6 sample) 기반 단위 test 추가.
- counter reset 시나리오 test.

## 3.3 Slice 2 — ROI loop closure (Claude G3 / Codex P1 ROI)

> `/compact` 또는 profile 전환 *수락 후*의 절감량을 화면에 직접 보여준다. 운영자 incentive 회복.

**대상 코드**:

- `src/store/recommendation_lifecycle.rs` — outcome 추적은 이미 있음.
- `src/store/insights.rs` — payoff 쿼리 추가.
- `src/insights_report.rs` 또는 alerts panel 한 줄.

**작업**:

1. lifecycle store SELECT + token_usage_samples JOIN: 추천 시점 ts0 ± N (예: 5분) vs ts0 + outcome_ts ± N의 input_tokens, cache_ratio, cost diff. **반드시 같은 pane_id로만 집계** (Slice 1 + Slice 0 전제).
2. 표시:
   - alerts panel 상단 “last action” chip: `last /compact 12m ago · saved ~8K input · cache cold→warm · $0.03 [Est]`.
   - Insights overlay 새 섹션 “Action Payoff” (action별 평균 절감 + sample size).
3. 추정 명시: `Estimated`, `Heuristic`. 전후 sample이 적으면 `n/a` (no data) 또는 `inconclusive` (significance 미달, threshold ≥ 5% 또는 1K).
4. profile switch 추천 후 token slope/cost slope 감소 여부도 같은 흐름으로 추적.
5. **outcome 카테고리 분리**: cache hot 상태에서 `/compact`를 _피한_ 경우(올바른 행동)는 “avoided」 outcome family, cache cold에서 `/compact` *수락*은 “accepted” family. accept rate가 같아도 두 family는 운영적 의미가 다름 (Codex 제안).
6. **Rule tuning candidates**: accepted_rate ≥ 50%지만 metric 개선이 inconclusive인 rule, 또는 ignored_rate ≥ 50%이고 false-positive 의심 rule을 “rule tuning candidates” 목록으로 노출.

**Done-when**:

- 4 시나리오 (saved / neutral / regressed / no data) + 2 family (avoided / accepted) test.
- significance threshold + sample minimum honor.
- alerts panel chip + Insights 섹션 동시 표출.

## 3.4 Slice 3 — Anomaly time-normalization + evidence enrichment (Codex P1 + Claude G4)

> cost-slope/token-slope를 sample timestamp 차이로 재계산. evidence를 UI에 풀어 보여줌.

**대상 코드**:

- `src/policy/rules/anomaly.rs:552, :576` (cost slope 5초 하드코딩).
- 동등하게 token-slope detector.
- `src/store/anomaly_sink.rs` — schema에 evidence 영속화 컬럼 추가.
- `src/ui/anomaly_overlay.rs` — row inline expansion.

**작업**:

1. cost-slope/token-slope의 elapsed_seconds 계산을 `sample.ts_unix_ms` 차이로. window_polls는 sample count로만 의미를 가지게.
2. sparse window 처리: `coverage_pct`, `elapsed_secs`, `sample_count` 3개를 evidence에 포함. coverage가 낮으면 detector confidence를 1단계 다운.
3. memory growth: absolute MB뿐 아니라 baseline 대비 비율 + provider별 정상 변동폭을 evidence에 넣음.
4. error burst: dominant kind, 최근 command, provider state를 evidence에.
5. SubagentSideEffect: co-occurring anomaly 종류 + 시간 간격을 evidence에.
6. **schema migration**: `anomaly_events` 테이블에 `evidence_json` 컬럼 (nullable) 추가. 기존 row는 NULL — read 측 fallback (`reason` 컬럼만 사용). additive migration.
7. UI: `n` overlay row 선택 시 ‘e’ 키로 inline 1–3 evidence sub-row 펼치기. row 높이 변화 + scroll 영향 test 필수.

**Done-when**:

- 시간 정규화 단위 test (poll interval 5s vs 10s vs 변동 fixture).
- evidence schema migration test (기존 NULL row 정상 read).
- inline expansion toggle test.
- coverage-low → confidence-down regression test.

## 3.5 Slice 4 — Now strip / Next-best-action (Claude G1/G2)

> Front page에 “지금 한 가지만 한다면” 한 줄 + Insights overlay 최상단 prescriptive 섹션.

(r1 Slice A + Slice B 통합. 알고리즘은 r1과 동일하지만 _Slice 0 이후_ attribution이 잠긴 후에만 의미 있음.)

**우선순위 priority queue**:

1. PermissionWait/InputWait 있으면 첫 번째.
2. Severity ≥ Risk strong rec.
3. quota 5h ≥ 0.85 또는 cost 80%.
4. 최근 1분 내 promoted=true anomaly.
5. healthy fallback.

**Insights NBA**:

- 지난 24h ignored strong rec 1개 골라 reason/suggested_command + (Slice 2에서 계산된) 비슷한 과거 행동의 평균 payoff 같이 표시.

**Done-when**:

- 5 priority 분기 test + 빈 reports edge case.
- IdentityConfidence::Conflict pane은 NBA 후보에서 자동 제외 (Slice 0과 결합).

## 3.6 Slice 5 — ETA chips with sample-timestamp normalization (Claude G5, Codex 시간 정규화 교훈)

> CTX/5H/7D/cost가 threshold 도달 ETA. 단, **sample timestamp로** 정규화 (poll count 아님).

**대상 코드**: 새 helper `src/policy/eta.rs` (pure compute).

**알고리즘**:

1. 최근 N(예: 12) sample의 (ts_unix_ms, value) pair에서 linear slope.
2. slope > 0이고 threshold(0.85) 도달 시간이 60분 이내면 chip 표시.
3. `[Est]` 라벨 강제.
4. R² 또는 std dev 신뢰도 ≥ threshold일 때만 표시.
5. sample interval이 갑자기 늘어난 경우 (pause/idle) ETA suppress — Slice 3의 coverage_pct 재사용.

**위험**: ETA가 *틀린 pane*에 표시되면 Slice 0 위반. 따라서 chip은 IdentityConfidence::Conflict pane에서 자동 hide.

**Done-when**:

- 6 fixture (rising / falling / flat / noisy / sparse / threshold passed) test.
- Conflict pane suppress test.

## 3.7 Slice 6 — Live smoke validation enrichment (Codex P2)

> `docs/ai/VALIDATION.md`와 `--once` fixture를 라이브 검증 중심으로 보강.

**작업**:

1. `docs/ai/VALIDATION.md`에 “Live identity matrix” 섹션 추가:
   - 시나리오: provider × wrapper(`node`/`bash`/none) × stale title × sidefile presence.
   - 각 셀의 기대 결과 (Confidence + suppressed metrics).
2. `tests/integration/once_fixtures/`에 새 황금 파일 3종:
   - `node_wrapper_gemini.txt`
   - `stale_title_qmonster.txt`
   - `ambiguous_claude_sidefile.txt`
3. CI 또는 release-time `--once` 출력을 fixture와 비교. drift 시 `cargo test` 실패.
4. canonical docs(`docs/ai/`)는 안정화된 결론만 승격, 실험 기록은 모델 plan에.

**Done-when**:

- VALIDATION.md identity matrix 추가.
- `--once` fixture 비교 test 통과.

## 3.8 Slice 7 — Sandbox/approval/audit visibility (Claude G6)

(r1과 동일. 단, Slice 0 IdentityConfidence::Conflict 4-state pattern을 그대로 재사용해 sandbox/approval도 4-state로 통일.)

## 3.9 Slice 8 — Cost breakdown + cross-agent duplicate hint (Claude G7 + Gemini "Cross-Agent")

> Insights overlay에 cost를 pane/model/situation으로 분해 + 같은 prompt가 여러 pane에서 중복으로 들어간 정황을 한 줄 hint.

**작업**:

1. r1 G7 cost 분해 (pane/model/situation) — Slice 1 pane bucketing 위에서 자연스럽게.
2. Cross-agent duplicate hint: 같은 5분 window에서 여러 pane이 *비슷한 token 증가 패턴*을 보인 경우 “possible duplicated context: Codex review + Claude main both showed +5K input within 2 min — consider sharing checkpoints”라는 advisory 한 줄. 새 detector가 아니라 Insights timeline의 후처리.

**위험**: false positive. 회피: minimum 같은 prompt 길이 추정 + 시간 정렬 + 사용자 dismiss.

## 3.10 Slice 9 — Dynamic layout / responsive auto-collapse (Gemini)

> 작은 viewport / 활성 modal 수 / focus에 따라 secondary 정보 자동 접기.

**대상 코드**: `src/ui/dashboard.rs` `dashboard_rects` + 카드 expanded/collapsed 분기.

**작업**:

1. width < 100 cells: 카드의 secondary signal chip row + runtime row 자동 접기 (toggle 키 `c` 제공).
2. 활성 modal 1개 이상: alerts panel 줄임.
3. focus = panes일 때 alerts panel 자동 축소 (현재 split %는 수동).
4. 항상 보일 minimum: title + state badge + ★p/★y chip + 1 metric row.

**위험**: 자동 접힘이 노이즈가 되어 정보 누락 인지 실패. 회피: 접힘 시 “3 rows hidden — press c to expand” chip.

## 3.11 슬라이스 의존성 (r2 갱신)

```
Slice 0 (Attribution Lock) ── 다른 모든 슬라이스의 전제. 첫 PR.
                                   │
                                   ▼
Slice 1 (Pane-bucket insights) ─── Slice 2, Slice 8의 전제.
                                   │
                                   ▼
Slice 2 (ROI loop)             ─── 운영자 incentive 회복.
Slice 3 (Anomaly normalize)    ─── 독립이지만 Slice 0 후가 안전.
                                   │
                                   ▼
Slice 4 (Now strip / NBA)      ─── Slice 2의 payoff 데이터를 활용.
Slice 5 (ETA)                  ─── Slice 0 conflict suppress 의존.
Slice 6 (Live smoke valid)     ─── Slice 0 동시에 진행 가능.
Slice 7 (Sandbox/audit)        ─── Slice 0 4-state pattern 재사용.
Slice 8 (Cost breakdown + dup) ─── Slice 1 의존.
Slice 9 (Dynamic layout)       ─── 마지막. cosmetic.
```

권장 순서: **0 → 6 → 1 → 2 → 3 → 4 → 5 → 7 → 8 → 9**.

(Slice 6을 0과 함께 일찍 — fixture가 0의 회귀 안전망이 됨.)

---

# 4. 검증 계획 (r2 갱신)

## 4.1 각 슬라이스 done_when

| Slice                    | done_when                                                          |
| ------------------------ | ------------------------------------------------------------------ |
| 0 Attribution Lock       | descendant exe walk + 3 회귀 fixture + 4-state suppress test 통과  |
| 1 Pane-bucket insights   | pane-grouped fixture (3 pane × 6 sample) test + counter reset test |
| 2 ROI loop               | 4 outcome × 2 family fixture + significance threshold test         |
| 3 Anomaly time-normalize | poll interval 변동 fixture + schema migration backward-compat      |
| 4 Now strip / NBA        | 5 priority + Conflict 자동 제외 test                               |
| 5 ETA                    | 6 fixture + Conflict suppress test                                 |
| 6 Live smoke validation  | VALIDATION.md matrix 추가 + 3 `--once` fixture 비교                |
| 7 Sandbox/audit          | 3 provider × 2 state + 4-state pattern 재사용                      |
| 8 Cost breakdown + dup   | 3 grouping × 2 data + duplicate-hint negative test                 |
| 9 Dynamic layout         | 3 viewport size × 2 modal count test + collapse-restore round-trip |

## 4.2 회귀 안전망

- 1445 lib + 65 integration test 그대로 유지.
- fmt + clippy clean.
- **새 invariant 1개**: `IdentityConfidence::Conflict pane은 어떠한 official metric badge도 표시하지 않는다`. property test.
- **새 invariant 2개**: `Insights aggregation은 동일 pane_id 내 sample만 first/latest로 짝짓는다`. property test.
- **새 invariant 3개**: `cost-slope/token-slope의 elapsed는 ts_unix_ms 차이만으로 계산한다`. test in detector.

## 4.3 Live dogfooding

- Slice 0 + 1 + 2가 머지된 후 1주간 chquan 본인 사용에서 (a) 식별 충돌 alert 발생 여부, (b) ROI payoff chip 표시 여부 측정.
- Slice 3 + 5 머지 후 1주간 ETA 정확도 (예측 vs 실제 도달) audit_events에 수집.

---

# 5. 위험 + 완화 (r2 갱신)

| 위험                                             | 완화                                                                                       |
| ------------------------------------------------ | ------------------------------------------------------------------------------------------ |
| Slice 0 descendant walk이 /proc race             | tick 단위 cache + invalidate-on-change                                                     |
| Slice 0이 너무 보수적이어서 정상 pane도 Conflict | 충돌 정의 = title.provider != exe.binary AND 둘 다 high-confidence; 그 외에는 Low로만 강등 |
| Slice 1 pane-bucket migration이 기존 row 처리    | additive 컬럼 + read-side fallback (Slice 3 schema migration 패턴 그대로)                  |
| Slice 2 payoff 추정이 noise                      | minimum sample + significance threshold + `inconclusive` 명시                              |
| Slice 3 evidence schema migration                | additive (`evidence_json` nullable) + read-side reason fallback                            |
| Slice 5 ETA가 잘못된 pane에 표시                 | Conflict pane 자동 suppress (Slice 0 결합)                                                 |
| Slice 9 자동 접힘이 정보 누락                    | “3 rows hidden — c to expand” chip + collapse 상태 audit                                   |
| 자동 actuation으로의 미끄러짐                    | Gemini “비용 반응형 자동 전환” 채택 안 함, 제안까지만                                      |

---

# 6. cross-validation 합의/상충 표 (요약)

| 사안                  | Claude r1                    | Codex P0/P1           | Gemini                | r2 결정                                                                                     |
| --------------------- | ---------------------------- | --------------------- | --------------------- | ------------------------------------------------------------------------------------------- |
| 최우선 과제           | Front page 합성 (G1/G2)      | Identity 귀속 lock    | Deep dive view        | **Codex 채택** — 귀속 buggy면 합성도 의미 없음                                              |
| Insights 통계 신뢰도  | 정직하지만 prescriptive 아님 | pane 섞임 발견        | 매우 신뢰             | **Codex 채택** — Gemini는 라이브 미확인                                                     |
| ROI 측정              | G3 “payoff 부재”             | P1 “ROI loop 부재”    | “비용 반응형”         | **3-way 합의로 Slice 2**                                                                    |
| 시간 단위 정규화      | r1 미확인                    | P1 cost-slope 5s 발견 | 미언급                | **Codex 채택, Slice 3**                                                                     |
| 자동 actuation 추가   | 금지                         | 금지                  | 일부 자동 전환 제안   | **Claude/Codex 합의로 거부**, Gemini 제안은 *제안 수준*으로만 채택                          |
| 새 modal 추가         | 금지                         | 미언급                | Deep dive (modal-ish) | **Claude 원칙 유지** — Deep dive는 기존 `n` overlay row inline expansion으로 흡수 (Slice 3) |
| Cross-agent token dup | 미언급                       | 미언급                | 새 detector 제안      | **detector 신설은 보류**, Insights timeline 후처리 hint로만 채택 (Slice 8)                  |
| Layout 반응형         | 미언급                       | 미언급                | 강조                  | **Slice 9로 후순위 채택**                                                                   |
| Live smoke validation | 미언급                       | P2 강조               | 미언급                | **Codex 채택, Slice 6**                                                                     |
| Sandbox/audit 가시성  | G6                           | 미언급                | 미언급                | **Claude 채택, Slice 7** (Slice 0 4-state 재사용)                                           |

---

# 7. 한 줄 결론 (r2)

> Codex가 라이브 스모크에서 잡은 **identity 귀속 버그**는 r1 평가의 전제(“수집된 공식 수치는 옳다”)를 깨고, *모든 후속 분석/표출/ETA의 신뢰성*을 흔든다. 따라서 r2는 **Slice 0 (Attribution Lock)을 다른 모든 슬라이스보다 먼저** 두고, Insights를 pane-bucket으로 재집계 (Slice 1), ROI loop 닫기 (Slice 2), anomaly 시간 정규화 (Slice 3) 순으로 진행한 뒤에야 합성·표출 슬라이스(4–9)에 들어간다. **새 데이터를 더 모으는 일은 한 슬라이스도 없고, 자동 actuation을 더하는 슬라이스도 한 개도 없다.**

---

# 8. 부록 A — r1 슬라이스와 r2 슬라이스 매핑

| r1                                          | r2                                       | 변화                                                                  |
| ------------------------------------------- | ---------------------------------------- | --------------------------------------------------------------------- |
| (없음)                                      | Slice 0 Attribution Lock                 | **NEW** — Codex P0                                                    |
| (없음)                                      | Slice 1 Pane-bucket insights             | **NEW** — Codex P1                                                    |
| Slice C payoff                              | Slice 2 ROI loop                         | 합의 + outcome family 분리 (Codex 제안) + rule tuning candidates 추가 |
| Slice D anomaly evidence                    | Slice 3 Anomaly normalize + evidence     | 시간 정규화 추가 (Codex) + Gemini Deep dive 흡수                      |
| Slice A Now strip + Slice B NBA             | Slice 4 Now strip / NBA 통합             | Slice 0 Conflict 자동 제외 추가                                       |
| Slice E ETA                                 | Slice 5 ETA                              | sample timestamp 정규화 강제 + Conflict suppress                      |
| (없음)                                      | Slice 6 Live smoke validation            | **NEW** — Codex P2                                                    |
| Slice F sandbox                             | Slice 7 Sandbox/audit                    | Slice 0 4-state 재사용 명시                                           |
| Slice G cost breakdown + Slice H cross-pane | Slice 8 Cost breakdown + cross-agent dup | Gemini cross-agent 흡수, dedicated detector 거부                      |
| (없음)                                      | Slice 9 Dynamic layout                   | **NEW** — Gemini                                                      |
| Slice I audit chip                          | Slice 7에 흡수                           | r1보다 가벼움                                                         |

---

# 9. 부록 B — peer plan 전문 인용 키 포인트

## Codex (`docs/codex/improvement_plan.md`)

> "라이브 스모크에서 `node /usr/bin/gemini --yolo` pane이 한때 `Claude`로 분류되어 Claude sidefile의 공식 token/cost/cache/reset 값이 붙었고, 현재 라이브 pane에서도 `cmd: qmonster`인 monitor pane이 canonical title 때문에 `Gemini research`로 표시된다. 즉 수치 자체는 공식 출처일 수 있어도 귀속 대상이 틀리면 화면의 신뢰도가 무너진다."
>
> "anomaly cost slope는 5초 polling interval을 코드에 고정한다(`src/policy/rules/anomaly.rs:552, :576`)."
>
> "insight cache/token growth는 전체 `token_usage_samples`를 시간순으로 훑어 first/latest input을 잡기 때문에 pane/provider가 섞일 수 있다(`src/store/insights.rs:502, :558`)."

→ Slice 0/1/3로 채택.

## Gemini (`docs/gemini/improvement_plan.md`)

> "특정 Metric이나 Anomaly 선택 시, 에이전트가 남긴 원본 로그 조각을 Diff 형태로 즉각 보여주는 'Deep Dive' 뷰" — Slice 3 inline evidence로 흡수.
>
> "동일 프로젝트 내 에이전트 간 중복 프롬프트 및 캐시 낭비를 식별하는 'Global Token Duplicate Detector'" — Slice 8 cross-agent hint로 흡수 (detector 신설 거부).
>
> "비용 소진 속도가 가파를 경우 자동으로 저비용 고압축 프로필로 전환을 제안하거나 자동 실행" — *제안*까지만 채택 (자동 실행 거부, spec §9.1 Layer 5 위반).
>
> "터미널 해상도와 활성화된 오버레이 개수에 따라 중요도가 낮은 정보를 자동으로 접어주는 반응형 레이아웃" — Slice 9로 채택.

---

_(end of r2)_
