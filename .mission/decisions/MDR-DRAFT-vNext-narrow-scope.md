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
