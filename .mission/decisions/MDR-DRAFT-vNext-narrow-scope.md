# MDR-DRAFT — vNext narrow scope (cut / keep)

> 상태: DRAFT (사람 승인 대기) · 날짜: 2026-06-30 · 소유자: chquan
> 결정 근거: `.docs/claude/Qmonster-2026-06-30-existence-vs-builtin-clis-crosscheck.md` (통합추천 = narrow)
> 브랜치: `narrow-vnext` (main/v2.8.0 불변 — 모든 cut은 git+v2.8.0 릴리스에 보존되어 가역)
> 교차검증: Codex (§5, contract = Codex + human) — 이 draft 검토 대기

## 1. 결정

Qmonster vNext를 **narrow**로 재구성한다: provider-neutral 코어(identity·cross-pane·핵심신호·SourceKind·audit·observe/recommend-only)는 지키고, provider별 신호 추격·광범위 token-opt 규칙·UI 모달 sprawl·n=1 polish는 버린다. UI/UX는 대안 B+(Two-Pane Core + 오버레이 급축소 + `--once` 다이제스트) — `UX_ALTERNATIVES.md` 참조.

## 2. 선택지 (요약)

- **A. status-quo(keep)** — 유지비/추격 treadmill 과다. 기각.
- **B. narrow(축소+얇은레이어)** ← **채택**.
- **C. pivot(감독평면)** — 정체성 충돌. 기각.
- **D. retire/freeze** — 사용 데이터가 "거의 안 씀"이면 정직한 답. 보류(사용 데이터 미확정 — §6 리스크).

## 3. cut / keep / reduce 매트릭스 (모듈 단위)

### KEEP (대체불가 코어, ≈10k LOC)

| 모듈                                                                                                            | 근거(원칙)             |
| --------------------------------------------------------------------------------------------------------------- | ---------------------- |
| `domain/identity.rs` (IdentityConfidence·Conflict·descendant walk)                                              | pane identity = 코어   |
| `domain/signal.rs` 핵심필드(context_pressure·quota_5h/weekly_pressure·resets·idle·active_files) + `MetricValue` | 핵심신호               |
| `domain/origin.rs` SourceKind                                                                                   | 정직성 축              |
| `domain/audit.rs` + `store/audit.rs` + `store/sqlite.rs`                                                        | audit (raw-bytes 없음) |
| `policy/engine.rs` (+ ObserveOnly choke-point)                                                                  | observe/recommend-only |
| `policy/rules/{alerts,advisories,concurrent,idle,reset}.rs`                                                     | 핵심 신호→권고         |
| `policy/gates.rs` (핵심 임계값)                                                                                 | 운영자 튜닝            |
| `adapters/{mod,common,process_memory,agent_memory}.rs`                                                          | provider-neutral 추출  |
| `tmux/*`                                                                                                        | pane 관측 인프라       |
| `ui/provider_honesty.rs`                                                                                        | SourceKind 표시        |

### REDUCE (유지하되 fallback/핵심만 — keep-list #4)

| 모듈                                                                      | 조치                                                                |
| ------------------------------------------------------------------------- | ------------------------------------------------------------------- |
| `adapters/claude.rs` (1444)                                               | tail 스크래핑은 **핵심신호 fallback만**; sidefile(구조화)이 primary |
| `adapters/codex.rs` (1571)                                                | 동일 — rollout(구조화) primary, scrape fallback                     |
| `adapters/gemini.rs` (1903)                                               | `/stats`·`/model` 파서 복잡도 대폭 축소 → idle/context fallback만   |
| `adapters/claude_sidefile.rs` (419) · `codex_rollout.rs` (339)            | **KEEP**(narrow가 endorse한 structured-output 경로)                 |
| `ui/panels` pane 카드                                                     | 5섹션 → 3섹션(IDENTITY+NOW / PRESSURE / RECOMMENDATIONS)            |
| `S` Settings                                                              | 5탭 → Thresholds + Integrations (Rules/Badges → 문서/`?`)           |
| `P` Provider Setup                                                        | 5탭 동적 → 정적 1화면 setup 안내                                    |
| `t` Target Picker · Action Explainer                                      | 최소 형태 유지                                                      |
| `policy/rules/{cache,profile_switch,anomaly,auto_memory,agent_memory}.rs` | 코어 신호 기반은 유지, provider-flavored 가지 정리                  |

### CUT (signal-chasing / sprawl / polish)

| 모듈                                               |  LOC | 근거                                                                                               |
| -------------------------------------------------- | ---: | -------------------------------------------------------------------------------------------------- |
| `policy/rules/profiles.rs` (3×2 grid)              | 2513 | "광범위 token-opt 규칙" — token-opt는 설계축이지 기능 아님                                         |
| `adapters/codex_app_server.rs` (HTTP quota client) |  638 | Codex-only 신호 추격, 무거움                                                                       |
| `adapters/agy_{footer,sidefile,transcript}.rs`     |  647 | agy enrichment = 추격 treadmill(교차검증서 명시). agy는 ObserveOnly 식별만 유지. **v2.8.0에 보존** |
| `ui/metrics.rs` 오버레이                           | 2616 | pane 카드 PRESSURE/RUNTIME와 중복                                                                  |
| `ui/insights.rs` + `store/insights.rs` 오버레이    | 795+ | "광범위 token-opt" 분석 표면                                                                       |
| `ui/anomaly_overlay.rs` (별도 오버레이)            | 1071 | anomaly *탐지*는 유지하되 표면은 Alerts로 흡수                                                     |
| `ui/pending_actions.rs` 배치 오버레이              | 1875 | observe/recommend-first — 멀티셀렉트 actuation 비핵심                                              |
| `ui/fx.rs` (confetti/matrix/fireworks…)            |  546 | 순수 n=1 polish — 버리는 것의 교과서                                                               |
| Hover Help ko/en·label/row 모드                    |    — | polish → 단일 `?`로                                                                                |

## 4. 비핵심 제거의 안전 원칙

- 모든 cut은 `narrow-vnext`에서 의미단위 커밋으로 — git·v2.8.0에 보존(가역).
- ObserveOnly·SourceKind·audit 계약은 cut 과정에서 byte-단위 보존(회귀 테스트 게이트).
- 각 슬라이스 후 `cargo test --all-targets` + fmt + clippy 게이트(프로젝트 규칙: shared-struct 변경은 --all-targets).
- 삭제로 테스트가 줄어드는 건 정상 — 무엇이/왜 줄었는지 커밋 본문에 기록.

## 5. 예상 효과

- LOC: ≈40,700(ui+app) + 코어 → narrow 후 **대략 절반 이하** 목표(정밀치는 구현 중 측정·기록).
- 표면: 오버레이 9+5 → **2~3**(`?` Help + 최소 `S` + 선택적 Git). FX/Metrics/Insights/Anomaly-overlay/Pending-batch/Provider-Setup-5탭 제거.
- 유지축: alert 큐 + pane identity/conflict + 핵심신호 + SourceKind + audit + observe/recommend-only.

## 6. 남은 리스크 / 미해결

- **사용 데이터 미확정**(원 결정 §1.4 5문 미답) → narrow vs retire/freeze 경계 불확실. narrow는 retire의 안전한 전단계(언제든 freeze 가능).
- A1(재설계 해석) 게이트 — `UX_ALTERNATIVES.md §5`.
- 버전 번호(v3.0.0 major 추정) — 릴리스 시 사람 승인 게이트.

## 7. 승인 / 재검토

- 승인자: chquan (대기)
- 재검토 조건: Codex 교차검증 결과 + 4주 narrow 사용 후 retire/freeze 판단.

## 8. Codex 교차검증 반영 + 스택 확정 + 실행 순서 (2026-06-30)

### 8.1 스택 결정 — **S1: Rust+ratatui 유지·축소** (`STACK_DECISION.md`)

사용자 지시("기존 설계/언어/라이브러리 무관, 오직 이유가 타당하면")에 따라 스택/패러다임 5대안(S1 Rust유지 / S2 Go+BubbleTea / S3 Py+Textual / S4 웹대시보드 / S5 헤드리스 thin-layer)을 3개 독립 리서치로 조사. **전부 "테스트된 코어 축소"로 수렴** — S1=71/80 압도. 재작성 기각(1642-테스트 moat 폐기 = narrow 정반대, §4.2 입증책임 미충족). S5(thin-layer)는 *어댑터 획득 원칙*으로 S1에 흡수. S4(웹)는 미래 read-only 추가뷰로만 유보.

### 8.2 Codex(reduction) approve-with-fixes 반영 — §3 보정

- **MUST-FIX#1 (adapter 재분류 — §3 REDUCE 행 override):** "구조화 primary, scrape fallback"은 **Claude만** 참(sidefile이 context/quota/cost/reset 제공). **Codex/Gemini의 context/quota/idle은 tail 파서가 _유일 core 경로_** → **KEEP**(축소 금지). Codex rollout은 token/model/window만 backstop. Gemini는 구조화 채널 없음. ⇒ 어댑터 cut은 *enrichment 추격분*만(codex_app_server, agy 3종, gemini `/stats`·`/model` 인터랙티브 파서). 핵심신호 scrape는 보존.
- **MUST-FIX#2:** metrics/anomaly 오버레이 제거 _전에_ 대체표면 정의 — metrics의 trend/slope-ETA → pane PRESSURE 카드 또는 `--once`; anomaly → Alerts 내 "recent anomaly strip"/digest history(탐지는 유지, 표면만 이전).
- **MUST-FIX#3:** `profiles.rs` 삭제 = **domain/UI/schema 마이그레이션**(`Recommendation.profile` 필드 + `domain/profile.rs` + `ui::panels::format_profile_lines` + settings profile config 동반 제거), 단일 파일 cut 아님.
- **under-cut 정련:** `auto_memory`/`agent_memory`/`cache`/`profile_switch`는 token/provider-tuning 성격 → **off-by-default/opt-in** 후보. `insights_report` CLI/store/lifecycle는 오버레이 cut 후에도 남음 → scope 명시. settings write-back은 read-only+config-path 안내로 대체 가능.

### 8.3 UI 재설계 확정 — pane 카드 **4섹션**

Codex 지적(5→3은 WHERE/RUNTIME 흡수처 불명확)에 따라 **4섹션: IDENTITY / NOW / PRESSURE / NEXT** 채택(scanability 보존). 오버레이는 `?` Help + 최소 `S`(Thresholds/Integrations) [+ 옵션 Git]. `--once` 다이제스트 유지·강화.

### 8.4 실행 순서 (Codex 권장, 의존성-안전, 채택)

1. **Inventory lock** — 핵심신호 golden 테스트 추가/고정(Claude sidefile, Codex statusline+rollout, Gemini status/stats가 context/quota/idle/permission/SourceKind 보장). _안전망 먼저._
2. **Profiles slice** — `engine.rs:52` eval_profiles 제거 → `Recommendation.profile`/`domain/profile.rs`/`format_profile_lines`/settings 제거. 매 단계 `--all-targets`.
3. **UI shell slice** — footer/help/keymap에서 cut 키 제거 → render 진입점 차단 → app overlay state 제거 → UI 파일 삭제(순서 중요: 파일 먼저 지우면 import/handler 깨짐).
4. **Metrics/anomaly 흡수** — trend/ETA/history/evidence 최소 대체표면 이식 후 오버레이 제거.
5. **Adapter narrow** — provider별 1개씩. Codex tail=core KEEP+rollout backstop 명명; Gemini status KEEP+`/stats`·`/model` 선택 cut; Claude sidefile primary+tail fallback.
6. **Settings/provider-setup** — Thresholds/Integrations만, 단 sidefile/rollout/status 가용성 diagnostic 유지.
7. **Final prune** — FX(wiring 먼저), insights overlay/store lifecycle, pending batch, hover 양언어 모드 제거 → 최종 fmt+clippy+test.

각 슬라이스: `cargo test --all-targets` + `cargo fmt --check` + `cargo clippy --all-targets -- -D warnings -A clippy::uninlined_format_args` green. 베이스라인 = **1642 lib + 140 int/supp**.

### 8.5 교차검증 상태

- reduction plan: **Codex approve-with-fixes (82/100)** — `.mission/evals/Qmonster-vNext-2026-06-30-narrow-redesign-codex-review.result.yaml`.
- 스택 결정: 3개 독립 리서치 수렴 + Codex 1회 확인(클린룸-rust-rewrite vs 축소 갭 표적) 진행.
- 사람 승인 대기: 버전번호(v3.0.0?) · A1 방향 · push/tag/release/publish.
