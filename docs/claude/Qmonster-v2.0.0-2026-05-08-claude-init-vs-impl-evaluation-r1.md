---
title: Qmonster v2.0.0 — `.docs/init/` 원안 vs 실제 구현 평가 + 개선 계획
author: Claude (Opus 4.7, 1M context)
date: 2026-05-08
version_at_evaluation: v2.0.0 (commit 5456934)
related_init_docs:
  - .docs/init/Qmonster_종합_기획_보고서_v0.4.0_2026-04-20.md
  - .docs/init/Qmonster_ms_init_prompt_ko_v0.4.0_2026-04-20.txt
  - .docs/init/Qmonster_토큰_최적화_조사_및_구현_보고서_2026-04-19 2.md
  - .docs/init/qmonster_policy_example.toml
status: r1 — 단독 작성 (codex/gemini 교차 검증 전)
---

# 0. 요약 (Executive summary)

> **결론.** 핵심 데이터는 **실제로** 수집·저장·표출되고, 토큰 최적화·이상 징후·감사 라인은 spec보다 **더 깊게** 쌓여 있다. 다만 사용자(운영자) 관점에서 화면이 “한눈에 의미 있는가”라는 질문에는 **부분적으로만 ‘예’**다. 과제는 “더 많이 보여주기”가 아니라 **모은 데이터를 한 화면 위에서 인과적으로 연결해 주기**다.

| 평가 축 | 평가 | 근거 |
|---|---|---|
| (1) 화면이 의미 있는 정보를 주는가 | △ | 알림/상태/식별/메트릭은 잘 노출. 그러나 7개 modal에 정보가 분산되어 한눈 운영이 깨짐 (`src/ui/dashboard.rs`, modal stack `m i n S P t ? K g`) |
| (2) 분석·통계·시각화가 의미 있는가 | ○ | Metrics overlay는 hottest banner + 24-cell pressure bar + sparkline + trend arrow까지 충실 (`src/ui/metrics.rs`). 그러나 Insights overlay는 사실상 “감사 집계표”로, 다음 행동을 안 알려줌 |
| (3) 데이터가 제대로 수집·표출되는가 | ◎ | Claude statusline / Codex app server / Gemini status table / pricing.toml / SourceKind 라벨 / SQLite 7개 테이블 + 보존 정책까지 모두 작동 (`src/adapters/`, `src/store/`) |
| (4) 토큰 최적화가 진짜 도움 되나 | △ | 14개 rule + 8개 anomaly detector + Notify Severity gate + ArchiveLocal effect로 “권고”는 정확. 그러나 “수락하면 얼마 줄었는지” payoff visualization 부재. 운영자는 자기 행동의 효과를 못 봄 |
| (5) 이상 징후 인사이트가 도움 되나 | △ | 8개 detector × 3 confidence × 5 severity, 영속 저장·promotion gate까지 정밀. 그러나 anomaly 1건의 “원인→결과 narrative”는 사용자가 직접 modal 사이를 점프하며 재구성해야 함 |

요점: **수집은 충분, 표출은 dispersed, 인사이트는 reactive — predictive·causal narrative를 한 화면에 돌려놓는 것이 다음 우선순위**.

---

# 1. 평가 방법

## 1.1 비교 대상

- 원안: `.docs/init/Qmonster_종합_기획_보고서_v0.4.0_2026-04-20.md` (총 1144줄, 2026-04-20 작성). 특히 §11(UI/UX), §12(보안·감사), §9(토큰 최적화 5층), §13(파라미터 분리), §15(개발 phase 1–5).
- 원안 보조: `Qmonster_ms_init_prompt_ko_v0.4.0_2026-04-20.txt` §8(UI/UX 요구), §11(요청 산출물).
- 구현 기준: 현재 `main` 브랜치, tag `v2.0.0`, 1445 lib tests passing.
- 핵심 코드 위치: `src/{ui,app,policy,store,adapters,domain}/`.

## 1.2 evidence 수집 규칙

각 항목마다 (a) 원안 인용, (b) 코드/테스트 위치, (c) 실제 작동 여부를 함께 적었다. 추정에는 “est.”를 명시했다. spec에 없으나 구현된 항목은 “(spec 외)”로 표시했다.

---

# 2. 항목별 상세 평가

## 2.1 화면이 의미 있는 정보를 제공하는가

### 2.1.1 원안의 요구

원안 §11.1–§11.2:

> 메인 화면 우선순위: 알림창(Notify, stop, stopfail, input wait, permission wait, context pressure, security concern). 그다음 정보 패널: pane별 provider/model/role, current command/path/repo/branch, context usage, cost/token hint, output storm/repeated output, sandbox/approval/policy, skill/tool/memory/MCP activity, audit severity.

### 2.1.2 실제 구현

- **Alerts panel**: `src/ui/alerts.rs` (2293 lines). Notify/state/storm/permission 각각 카드화. focus + filter (`/`) + severity color + ★y yank chip.
- **Panes panel**: `src/ui/panels.rs` (3929 lines). 카드당 12+ 라인:
  - title (state badge + identity + CLI version + ★p chip)
  - state row (with flash pulse)
  - path / cmd / status
  - blocking signal chips (waiting for input, approval needed)
  - secondary signal chips (log storm, repeated output, verbose, error hint, subagent)
  - 2 metric badge rows (CTX/QUOTA/5H/7D/RESET/TOKENS/COST/MODEL/MEM/MEM-FILE/CACHE)
  - 토큰 sparkline status, token I/O, cache token I/O (expanded)
  - runtime badges (skill/tool/MCP)
  - top 3 recommendations + structured profile lines (lever/citation/SourceKind)
- **Footer**: `★p` (pending proposals) + `★y` (yank-able) + focus indicator + split %.
- **Modal stack**: `m`(Metrics) `i`(Insights) `n`(Anomaly) `S`(Settings) `P`(Provider Setup) `t`(Target picker) `?`(Help) `K`(Keys) `g`(Git).
- **Theme**: Dark / HighContrast / Light (v1.60.0).
- **FX**: 7개 scene + Sampler (v1.59.0). 여기는 spec에 없는 장식이지만 `[fx] effect`로 끌 수 있음.

### 2.1.3 평가

|항목 | 원안 요구 | 구현 | 비고 |
|---|---|---|---|
| Notify/stop/input wait/permission wait | 필수 | ✓ alerts.rs + IdleCause + PermissionWait | OK |
| context pressure / security concern | 필수 | ✓ context_pressure metric + security findings via concurrent.rs | OK |
| provider/model/role | 필수 | ✓ pane_panel_title + identity layer | OK |
| command/path/repo/branch | 필수 | ✓ panel cards + git_branch metric | OK |
| context usage | 필수 | ✓ CTX badge + Metrics overlay bars | OK |
| cost/token hint | 필수 | ✓ COST/TOKENS badges + token sparkline | OK |
| output storm/repeated output | 필수 | ✓ secondary signal chips + log_storm rule | OK |
| sandbox/approval/policy | 필수 | △ approval은 idle_state(PermissionWait)으로만 노출, sandbox 모드 자체는 별도 tile 없음 | **gap** |
| skill/tool/memory/MCP activity | 필수 | ○ runtime_facts + agent_memory_bytes로 부분 노출, MCP는 spec 외 | partial |
| audit severity | 필수 | △ severity color는 alerts에 노출되지만 “audit log 자체”의 mini-strip은 없음 | **gap** |
| official vs estimated 표기 | 필수 (§11.4) | ✓ SourceKind label `[Official]/[Heur]/[Est]/[Provider]/[Project]` 모든 metric badge에 부착 | OK |
| 저채도/회색·청색 팔레트 | 필수 (§11.3) | ✓ theme.rs + Light/HighContrast 변형 | OK |
| 한 번에 보기 좋다 (§4.1) | 가치 | △ 7 modal × 9 hotkey로 surface area가 spec 대비 확대됨. 첫 사용자가 “지금 뭘 봐야 하지” 결정에 30초+ 소요 (est., help modal 처음 펼치는 비용) | **gap** |

**판단**: 정보는 있지만 **분산되어 한눈 운영을 깨뜨림**. 원안의 “alert-first, 그다음 정보 패널”은 두 굵은 panel + 작은 footer 정도였는데, 1년 사이 7개 modal이 추가되며 “정말 중요한 한두 개 숫자”를 즉시 보여주는 표면이 사라짐.

## 2.2 분석·통계·시각화가 의미 있는가

### 2.2.1 Metrics overlay (`m`)

- `src/ui/metrics.rs` (2344 lines).
- **Hottest banner**: 모든 pane 중 CTX/5H/7D 기준 가장 압박받는 pane을 한 줄로 표시. 데이터 없으면 `Hottest: —` (정직).
- **Per-pane card** (4 row, left/right split):
  - **Left**: CTX / 5H(또는 QUOTA) / 7D / CACHE 24-cell `█/░` bar + severity color (Pressure) 또는 neutral (CACHE) + pct + trend arrow (▲/▼/─, v1.60.0).
  - **Right**: 5H/7D reset ETA combined / TOKENS in sparkline (8-glyph density, Δ+delta) / TOKENS out sparkline / COST + MEM + MEM-FILE combined row with trend arrows.
  - 카드당 anomalies 1줄 추가 (kind:confidence summary).

**평가**: ◎. 24×4 그리드에 의미 있는 시계열·압박·여유 시간을 동시에 본다. spec의 §11.3 “숫자 % + 등급 문자 같이 표기”도 만족. 운영자가 “어느 pane이 가장 심한가”를 1초 안에 파악 가능.

### 2.2.2 Insights overlay (`i`)

- `src/ui/insights.rs` (788 lines) + `src/store/insights.rs` (824 lines) + `src/insights_report.rs` (227 lines).
- 표출 항목 (`format_insights_report_lines`):
  - window: `since_ms..until_ms`
  - **Situations**: `Log storm/repeated output`, `Code exploration`, `Context pressure`, `Verbose review`, `Input/permission wait`, `Quota-tight/cost`, `Other`별 emit 카운트.
  - **Cache**: latest_cache_ratio %, token_growth Δ, cost_delta_usd, hot/cold/drift count.
  - **Action Ledger**: action 라벨별 emitted/accepted/rejected/blocked/completed/failed/archived/snapshot/hidden/ignored.
  - **Action Rates**: accepted_rate, completion_rate, ignored_rate.
  - **Recent Timeline**: ts/pane/situation/action → outcome.
  - **Evidence**: 어느 SQLite 테이블에서 왔는지 명시 (`audit_events`, `token_usage_samples`, `cost_usage_events`).
- staleness: 5분 후 stale chip, `r`로 refresh, refresh 시 SystemNotice toast.

**평가**: ○ + ▼.

- ○ 잘된 점: **데이터의 출처(`Evidence` 줄)와 `ignored_available` 상태를 정직하게 노출**. lifecycle store가 부재하면 “audit-only, recommendation correlation unavailable”이라고 명시. 이건 spec §11.4 “official vs estimated” 정신에 충실.
- ▼ 약한 점: **운영자가 다음에 뭘 해야 하는지가 빠짐**. “Cache reuse 42%, token growth +1234, cost delta $0.03” → 그래서? 다음 한 행동은? 현재 화면은 **post-hoc 집계**이고, **prescriptive (next-best-action)**가 없음. spec §9.2의 시나리오 A–F는 “신호 → 조치” 매핑이 명확했는데, Insights는 그 매핑을 드러내지 않음.
- ▼ 약한 점: **인과 narrative 부재**. timeline은 ts/pane/situation/action/outcome 5튜플인데, “14:03 cache drift → 14:05 /compact 권고 → 14:06 reject → 14:10 verbose-review burst” 같은 한 줄 인과 stitching이 없음.
- ▼ 약한 점: **cost 분해 부재**. cost_delta_usd는 총합. pane별/모델별/situation별 분해가 안 됨. $200 budget 사용자가 “Codex review가 60% 태웠다”는 결론을 못 내림.

### 2.2.3 Anomaly overlay (`n`)

- `src/ui/anomaly_overlay.rs` (968 lines) + `src/store/anomaly_sink.rs` (511 lines).
- 두 view: Ring (live signals) / History (SQLite-backed).
- Filter cycle: All → PromotedOnly → HighOnly.
- v1.59.0: row severity color coding.
- 8 detector × 3 confidence (Low/Medium/High) × 5 severity.
- promotion gate: 일부 anomaly는 Recommendation으로 graduate.

**평가**: ○. 영속 저장 + promotion gate + filter는 spec(§9.2 D-Anomaly)을 넘어선다. 그러나 **“anomaly 1건의 narrative”가 row 한 줄(kind:confidence:severity:promoted:reason)로 압축되어 운영자가 “왜 이게 문제인가”를 모름**. evidence는 detector 안에 잡히지만(metric_name/before/after/sample_count/source_kind), overlay에 풀어 보여주지 않는다.

### 2.2.4 종합 시각화 평가

- **시각화 자체의 품질**: ◎ (sparkline, severity bar, trend arrow, color, source label).
- **분석의 깊이**: ○ (8 detector + 14 rule).
- **운영자의 ‘다음 행동’ 도달성**: △. modal 3개를 점프해야 “지금 뭐할지”를 합성할 수 있음.
- **인과/예측**: ▼. timeline은 평탄, narrative 없음.

## 2.3 데이터가 제대로 수집·표출되는가

### 2.3.1 수집

- **Tmux polling**: `src/tmux/` + `src/app/polling_tick.rs`. control mode 확장 가능 추상화는 spec 충실.
- **Provider adapters** (`src/adapters/`):
  - `claude.rs` (2.3절): statusline 1줄 파싱 — model, reasoning_effort, ctx %, cache %, 5h %, 7d %, worktree path. confidence 0.95, SourceKind=Official. Statusline 부재 시 fallback parser. limit hit 텍스트 감지 + recovery hint anchor.
  - `codex.rs` + `codex_app_server.rs`: Codex CLI app-server prompt parsing.
  - `gemini.rs`: Gemini status table + RuntimeFactKind::ModelReset.
  - `claude_sidefile.rs`: side file (CLAUDE.md 등) 크기 측정.
  - `agent_memory.rs` + `process_memory.rs`: agent memory bytes + RSS.
  - `qmonster.rs`: 자기 자신 모니터.
- **Settings/config 수집**: `claude_settings.rs`로 Claude config 읽음.
- **Pricing**: `src/policy/pricing.rs` (248 lines) — pricing.toml + 90-day staleness 검사.

### 2.3.2 저장 (SQLite)

`src/store/sqlite.rs` + 7개 sink:
1. `audit_events` (`audit.rs`, 447 lines) — 모든 lifecycle 이벤트.
2. `token_usage_samples` (`token_usage.rs`, 324 lines) — 토큰 시계열 + cached_input_tokens (v1.30+).
3. `cost_usage_events` (`cost_usage.rs`, 273 lines) — USD delta.
4. `anomaly_events` + `anomaly_history_snapshots` (`anomaly_sink.rs`, 511 lines) — 8 detector 출력.
5. `recommendation_events` + `recommendation_outcomes` (`recommendation_lifecycle.rs`, 348 lines) — accept/reject/complete/fail/ignore.
6. `archive_fs` (`archive_fs.rs`, 204 lines) — raw tail archive 파일시스템.
7. `snapshots` (`snapshots.rs`, 118 lines) — 운영자 트리거 체크포인트.

`retention.rs` (208 lines): 시간 기반 + 행 수 기반 hard cap + 자동 purge.

### 2.3.3 표출

각 metric이 `MetricValue { value, source_kind, confidence, provider }`를 들고 다닌다. UI에서 모두 `[Official]/[Heur]/[Est]/[Provider]/[Project]` 라벨을 표시. spec §11.4 “official 수치와 estimated 수치를 구분”의 정신을 가장 정직하게 구현한 부분. provider-honesty 모듈(`src/ui/provider_honesty.rs`, 144 lines)이 4-state (Value / Pending / Unsupported / Hidden) 모델로 “모름” 자체를 1급 시민으로 다룸.

### 2.3.4 평가

◎. spec의 “덜 변하는 config (§13.1) vs 자주 변하는 runtime state (§13.2)” 분리가 코드와 schema 양쪽에 깔끔하게 살아 있음. 1445 lib test 통과로 invariant이 묶여 있음. **데이터 수집·저장 측은 spec을 초과 달성**.

## 2.4 토큰 최적화가 진짜 도움 되는가

### 2.4.1 spec의 5층 (§9.1)

| Layer | spec 요구 | 구현 위치 | 평가 |
|---|---|---|---|
| 1. Provider-native profile | low-token profile 추천 | `policy/rules/profiles.rs` + `provider_setup.rs` (snippets) | ✓ profile 페이로드(`format_profile_lines`)에 lever/key/value/citation/SourceKind 모두 노출 |
| 2. Observation | tail/adapter/pressure | `adapters/` + `signal.rs` SignalSet | ✓ 깊음 |
| 3. Archive/checkpoint | raw archive, preview/full split | `store/archive_fs.rs` + log_storm → ArchiveLocal | ✓ |
| 4. Policy/recommendation | 관찰·권고, quota-tight 분기 | `policy/engine.rs` + 14 rule + PolicyGates | ✓ |
| 5. Limited actuation | manual 우선, destructive 금지 | `PromptSendProposed` + 운영자 confirm | △ — 권고는 강력하지만 결과 측정이 부족 |

### 2.4.2 신호 → 조치 매핑 (spec §9.2 시나리오 vs 구현)

| 시나리오 | spec 신호 | 구현 rule | 작동? |
|---|---|---|---|
| A. log storm | 로그 비중↑·반복 출력·output chars 급증 | `rules/alerts.rs` log_storm + `rules/advisories.rs` aggressive | ✓ + Notify ≥ Warning gate |
| B. code exploration | 파일/심볼/검색 반복 | `rules/advisories.rs` code_exploration | ✓ |
| C. context pressure | usage hint, 긴 세션 | `rules/advisories.rs` context_pressure (`/compact` 강한 권고) | ✓ + `/compact` strong rec → PromptSendProposed graduation |
| D. verbose review | 장문 설명 반복 | `rules/advisories.rs` verbose_review | ✓ |
| E. permission/input wait | approve/wait | `IdleCause::PermissionWait/InputWait` + idle_state rule | ✓ + Warning severity |
| F. quota-tight | 5h/7d 정책 | `rules/reset.rs` + PolicyGates.quota_tight + aggressive variants | ✓ + 7d/5h reset eta surface |
| G. cache hot/cold/drift | (spec 외, F-7) | `rules/cache.rs` (recent_token_samples 기반 추론) | ✓ (구현이 spec을 초과) |
| H. cost budget 80%/100% | (spec 외, F-9b) | `rules/cost_budget.rs` | ✓ |

### 2.4.3 “정말 도움 되는가” 평가

- **권고의 정확성**: ◎. 14 rule이 SignalSet + PolicyGates + recent_token_samples + recent_errors를 입력으로 받아 다중 신호 결합. severity gating + quota_tight gating + identity_confidence gating 3중 gate로 노이즈 제거.
- **권고의 출처 honesty**: ◎. profile lever마다 citation + SourceKind. spec §9.1 “공식 근거 vs 휴리스틱 구분”의 가장 강력한 구현.
- **권고의 작동성**: ○. PromptSendProposed graduation으로 `/compact`를 한 키로 보낼 수 있음.
- **권고의 효과 측정**: ▼. **`/compact` 수락 후 “토큰 5K 절감, $0.04 saved, cache rebuilt to cold”의 payoff가 화면에 안 돌아옴**. lifecycle store에 raw 데이터(accepted → next sample)가 있으나, UI는 “accepted_rate %”라는 aggregated 비율만 보여줌. 운영자는 “내 행동이 효과 있었나”를 못 봄. 이게 toolset이 “진짜 도움 되는가”를 운영자가 체감하는 가장 큰 결정적 surface인데 아직 비어 있음.

## 2.5 이상 징후 인사이트가 도움이 되는가

### 2.5.1 detector 라인업

`src/policy/rules/anomaly.rs` (별도 파일, base) — 8 kind:
1. IdentityChurn — pane identity가 너무 자주 바뀜.
2. ErrorBurst — 짧은 시간 다수 error.
3. CacheDiscontinuity — cache_hit_ratio 급변.
4. CrossPaneEditCluster — 같은 path를 여러 pane이 동시 수정.
5. CostSlope — cost USD 가파른 상승.
6. TokenSlope — input_tokens 가파른 상승.
7. MemoryGrowth — RSS / agent_memory 성장.
8. SubagentSideEffect — subagent_hint와 다른 anomaly 동반.

각 detector는 `AnomalyEvidence { metric_name, before, after, sample_count, source_kind }`를 기록. window_polls + edge-triggered detected_at으로 dedup.

### 2.5.2 표출 (`n` overlay, panes 카드 ANOMALIES 줄, metrics 카드 anomalies 행)

- Ring (live) + History (SQLite, `fetch_recent_anomaly_events` 100K row hard cap).
- promotion gate: 일부는 Recommendation으로 graduate (예: "anomaly: cost slope detected" → cost-budget 계열).
- filter: All / PromotedOnly / HighOnly.

### 2.5.3 평가

- **수집의 깊이**: ◎. 8 detector + 영속 + retention + promotion까지.
- **표출의 정직성**: ○. spec §12.3 “severity 5단계”와 맞고, promoted 여부도 row에 명시.
- **운영자에 도움 되는가**: △. row를 보면 `CostSlope:high:warning:promoted:cost_usd at ts=…` 정도. **“왜 이게 발생했는지, 어떤 metric이 얼마만큼 변했는지, 어느 pane들과 상관 있는지”의 narrative가 부재**. evidence는 detector 안에 있는데 UI까지 안 흘러옴.
- **상관 관계**: ▼. 두 anomaly가 거의 동시에 다른 pane에서 발생하면 `SubagentSideEffect` detector가 그걸 잡지만, 시각적으로 “correlated”라고 보여주는 표면이 없음. CrossPaneEditCluster + concurrent rule이 있지만 행렬·매트릭스 시각화는 없음.
- **사후 vs 사전**: ▼. 모두 **이미 발생한** anomaly의 사후 표시. ETA 예측, “이 추세면 30분 안에 cost-budget 80% 도달 예상” 같은 forward-looking 표면이 없음.

---

# 3. 종합 진단

## 3.1 성공한 부분 (유지)

1. **Source-of-truth honesty 문화**: SourceKind/MetricValue/provider-honesty 4-state. spec의 정신을 가장 잘 살린 부분.
2. **수집·저장의 깊이**: 7 SQLite 테이블 + retention + 1445 test. Phase 2 “Archive/Checkpoint”와 Phase 3 “Policy Engine”이 완전히 작동.
3. **권고의 다중 gate**: severity ≥ Warning Notify, quota_tight aggressive variants, identity_confidence gating. noise control이 spec을 능가.
4. **관측·표출 분리**: signal layer가 pure하고, UI가 그 위에서 view를 합성. test 가능 + 변경 안전.
5. **Metrics overlay**: spec에는 없던 24-cell pressure bar + sparkline + trend arrow. **이 modal 하나는 “한눈 분석”을 거의 달성**.

## 3.2 부족한 부분 (개선 후보)

### G1. Front page “Now” 영역 부재 (spec §11.1 alert-first 의도 약화)

- **문제**: 운영자가 “지금 뭘 해야 하나”를 알기 위해 alerts panel + panes panel + 세 modal을 점프해야 함.
- **증상**: Help modal 없이 신규 운영자는 “★p, ★y, CTX, CACHE drift, ANOMALIES” 의미 학습이 필요. 한 번에 보기 좋은 상태가 아님.
- **원인**: 1년 동안 modal이 9개로 증가(`m i n S P t ? K g`). spec이 가정한 “2 panel + 작은 footer”의 단순함이 희석됨.

### G2. Insights overlay가 prescriptive하지 않음

- **문제**: action ledger·cache stat·timeline은 사후 집계. “지금 받아들여야 할 권고”가 빠짐.
- **증상**: `i` overlay를 열어도 “ok now what”이 안 나옴.

### G3. 행동의 payoff visualization 부재

- **문제**: `/compact` 수락 후 “얼마 줄었는지”의 before/after가 화면에 없음. 운영자가 “이 toolset이 효과 있나”를 직관적으로 체감 못 함.
- **증상**: 운영자 incentive 약화 → 권고 무시율 ↑ 가능성 (lifecycle outcome 데이터로 추후 검증 가능).

### G4. Anomaly narrative 부재

- **문제**: row 한 줄(kind:confidence:severity:promoted:reason)로 압축. evidence(metric/before/after/sample_count)가 store에 있는데 UI는 안 풀어줌.
- **증상**: anomaly 발생 시 “왜 이게 문제인가”를 운영자가 직접 metrics overlay·alerts·panes 카드를 점프하며 재구성.

### G5. Forward-looking surface 부재

- **문제**: 모든 surface가 현재·과거. ETA 예측, “이 추세면 X분 후 quota 80%” 표시가 없음.
- **증상**: 운영자는 quota 80% 도달 후에야 행동 → spec §9.2 F “quota-tight 사용자에게 더 적극적 권고” 정신과 어긋남.

### G6. Sandbox/approval/audit 가시성 부족 (spec §11.2 명시 항목)

- **문제**: approval은 `IdleCause::PermissionWait`로만 노출. sandbox 모드 자체를 한눈에 보는 tile 없음. audit severity는 alerts에 색으로만, 별도 strip 없음.
- **증상**: spec §12 “Qmonster가 기록할 감사 이벤트 9개”가 audit_events 테이블엔 들어가지만, 운영자 표면에는 없음.

### G7. Cost 분해 부재

- **문제**: cost_delta_usd는 단일 합계. pane / model / situation 분해 없음.
- **증상**: budget이 빨리 닳을 때 “어느 surface가 주범인가”를 쉽게 못 답함.

### G8. Cross-pane 상관 시각화 부재

- **문제**: G-11 concurrent rule + CrossPaneEditCluster + SubagentSideEffect가 발화하지만, 두 pane의 anomaly를 “같은 시점 같은 path”로 묶어 보여주는 vis(매트릭스/타임라인 stripe)가 없음.

---

# 4. 개선 계획 (구현 X — 계획 only)

## 4.0 우선순위 원칙

1. **새 데이터 수집을 더하지 말고 모은 데이터를 더 잘 보여주기** (spec §0 “토큰 최적화 by architecture”의 “구조 원리” 정신).
2. **Modal 더 늘리지 않기**. front-page 또는 기존 modal 안에서 surface를 옮기기.
3. **recommendation-first 유지**, automatic actuation 금지.
4. **operator confidence (정확도)와 effort (한 번에 보기) 둘 다 개선**.
5. **각 슬라이스는 1주~2주 분량의 단일 PR로 끝낼 수 있는 크기**.

## 4.1 Slice A — “Now” 통합 strip (front page)

> alerts panel 위 또는 안에 1–2 line의 **결정적 한 줄**을 추가. modal 없이 “지금 한 가지만 한다면”을 보여주는 surface.

- **표면**: `src/ui/dashboard.rs` `render_dashboard` 안에서 alerts rect 위 1 line 또는 alerts header subtitle.
- **표시 내용** (우선순위 순, 한 번에 1 항목):
  1. PermissionWait/InputWait가 있으면 “{pane} WAIT INPUT — Enter approve · q skip” (spec §11.1 최우선).
  2. Severity ≥ Risk 권고가 있으면 “{pane} RISK: {reason} — p send {slash} · d dismiss”.
  3. quota_5h_pressure ≥ 0.85 또는 cost_delta_usd가 budget 80% 초과면 “{pane} QUOTA 88% / cost burn ${rate}/hr — see m”.
  4. anomaly가 1분 이내에 promoted=true로 발화했으면 “{pane} ANOMALY {kind} ({confidence}) — see n”.
  5. 위 모두 없으면 “healthy · {n} panes · last anomaly {ago}”.
- **데이터 출처**: 기존 reports + AnomalyEventsRing + cost_usage_events 합산. 새 collection 없음.
- **테스트 전략**: 5개 priority 분기마다 unit test 1개 + 빈 reports edge case.
- **위험**: alerts 첫 줄과의 시각적 중복. 회피: alerts panel은 “open alerts 큐”, Now strip은 “the one thing now” — 표현 분리 (chip 형태 + dimmer 색).

## 4.2 Slice B — Insights overlay에 Next-best-action 섹션

> 현재 Insights overlay 최상단에 **운영자가 다음 5분 안에 할 한 가지 행동**을 prescriptive하게 노출.

- **표면**: `src/insights_report.rs` `format_insights_report_lines` 첫 섹션 “Now Suggested Action”.
- **알고리즘** (pure, deterministic, 새 데이터 없음):
  1. lifecycle store에서 지난 24h `RecommendationEmitted` 중 outcome != accepted 가장 최근 strong rec 1개 선택.
  2. 그 rec의 reason/situation/suggested_command/profile lever 첫 줄을 묶어 표시.
  3. 없으면 “no pending action” + “open ★p chip count” 표시.
- **위험**: prescriptive와 silently 자동화의 경계. 회피: 어디까지나 “suggested”, 키 누름은 운영자가.

## 4.3 Slice C — `/compact` payoff strip

> 토큰 최적화 행동 후 effect를 **숫자로 한 줄** 보여주는 후속 surface.

- **표면**: alerts panel 안 “last action” chip 또는 metrics overlay banner 다음 줄.
- **데이터**: lifecycle store에서 마지막 `PromptSendCompleted` 시점 직전 ±N tick의 input_tokens/cost/cache_ratio diff.
- **표시 예**: “last /compact 12m ago · saved ~8K input tokens · cache cold→warm · $0.03”.
- **계산 위치**: 새 helper `lifecycle::last_action_payoff(window)` (pure, store에서 SELECT만).
- **테스트 전략**: 가짜 lifecycle row + before/after sample fixture로 4 시나리오 (saved, neutral, regressed, no data).
- **위험**: payoff 계산이 단순 빼기여서 noise. 회피: minimum sample count 6 + significance threshold (예: |Δ| ≥ 5% 또는 1K tokens). 데이터 부족 시 “n/a” 표시.

## 4.4 Slice D — Anomaly evidence expansion

> `n` overlay row를 펼치면 detector가 잡은 evidence를 보여줌.

- **표면**: `src/ui/anomaly_overlay.rs`에 이미 Ring/History 두 view가 있음. 세 번째 view “Detail”을 추가하지 말고, **선택된 row 하단에 evidence sub-rows 1–3줄**을 inline으로 풀어 보여줌.
- **데이터**: AnomalyEvidence는 이미 detector → AnomalySignal → AnomalyEvent 흐름에 있음. 다만 영속화 schema에 evidence가 빠져 있음 (`anomaly_events` 컬럼: ts/pane_id/kind/confidence/severity/promoted/reason). evidence 영속화는 후속 slice에서.
- **예 표시**:
  ```
  CostSlope:high:warning:promoted | pane=Codex review
    cost_usd  $0.12 → $0.34 over 6 samples [Estimated]
    input_tokens 4.2K → 11K over 6 samples [ProviderOfficial]
  ```
- **위험**: row 높이 변화. 회피: filter처럼 ‘e’ 키로 토글, 닫으면 단일 line.

## 4.5 Slice E — Forward-looking ETA chips

> 현재 추세 기반 “{metric}가 {threshold}에 도달까지 {eta}” 추정.

- **표면**: panes 카드 metric badge row 끝에 작은 chip. metrics overlay에는 hottest banner 아래 한 줄.
- **알고리즘** (pure):
  1. 최근 N(예: 12) tick의 metric value 선형 회귀 (또는 단순 last-N slope).
  2. slope > 0이고 threshold(예: 0.85)까지 ETA가 60분 이내면 chip 표시.
  3. SourceKind는 입력 metric의 source_kind를 그대로 가져가되 “[Est]”로 강제 (예측은 항상 추정).
- **표시 예**: `CTX 78% [Official] · ETA 85% in ~14m [Est]`.
- **테스트 전략**: 6개 fixture (rising slope, falling slope, flat, noisy, insufficient samples, threshold already passed).
- **위험**: 잘못된 ETA가 더 큰 noise. 회피: minimum sample 6 + slope confidence threshold + “[Est]” 라벨로 운영자가 중요도 조정 가능. R² 또는 std dev 같은 신뢰도 측정 필수.

## 4.6 Slice F — Sandbox/Approval mini-strip

> spec §11.2에 명시된 항목. 1줄 strip 또는 panes 카드 한 row 추가.

- **표면**: panes 카드의 secondary signal chip row 옆 또는 metric row 끝.
- **표시 내용**:
  - approval 모드 (claude-settings.rs에서 읽음): `approval=auto/manual/once`.
  - sandbox/hooks 활성화 여부: `sandbox=on/off`.
  - 둘 다 모르면 `policy ?`.
- **데이터**: claude_settings + AGENTS.md/GEMINI.md hint. 새 수집 거의 없음 (이미 존재하는 settings reader 재사용).
- **위험**: provider별 설정 위치가 달라 false positive. 회피: SourceKind=Heuristic, `?`/`—` 4-state 그대로 사용.

## 4.7 Slice G — Cost breakdown surface

> Insights overlay에 cost_delta_usd 분해.

- **표면**: Insights overlay “Cache” 섹션 다음에 “Cost Breakdown” 섹션 추가.
- **데이터**: cost_usage_events JOIN audit_events on pane_id, kind grouping by (pane / model / situation).
- **표시 예**:
  ```
  Cost Breakdown (window total $1.24)
    by pane:    Codex review $0.74 · Claude main $0.38 · Gemini research $0.12
    by model:   gpt-5.5 $0.74 · claude-sonnet-4.6 $0.38 · gemini-3.1-pro $0.12
    by situation: Verbose review $0.62 · Code exploration $0.31 · …
  ```
- **위험**: pane/model/situation 라벨링이 일관되어야 함. 회피: 이미 `situation_for_action` 매핑이 lifecycle store에 있음 — 그대로 사용.

## 4.8 Slice H — Cross-pane correlation row

> 같은 분에 발생한 anomaly를 한 줄로 묶음.

- **표면**: Insights overlay “Recent Timeline” 다음에 “Cross-pane Correlations” 섹션, 또는 anomaly overlay History view 상단.
- **알고리즘**: ±60s window 안에 2개 이상 pane이 anomaly를 발화한 경우 한 묶음으로.
- **표시 예**:
  ```
  14:03  Codex(ErrorBurst:high) + Claude(CacheDiscontinuity:medium)  3 panes
  ```
- **위험**: spurious correlation. 회피: 최소 2 pane 필요 + same window_label 우선.

## 4.9 Slice I — Audit severity strip (선택)

> spec §11.2 “audit severity” 항목.

- **표면**: footer에 ★p / ★y 옆 ★a (audit) chip. 최근 N분 audit_events에서 max severity 표시.
- **데이터**: 이미 audit sink에 있음. 단순 SELECT max(severity).
- **위험**: 항상 색 들어와서 noise. 회피: severity ≥ Warning만 chip 색, 아니면 dim 표시.

## 4.10 슬라이스 간 의존성

```
Slice A (Now strip) ──┐
                       ├─ 모두 독립.
Slice B (NBA)        ─┤  Slice C는 lifecycle store에 PromptSendCompleted 데이터 필요 (이미 있음).
Slice C (payoff)     ─┤
Slice D (anomaly evidence) — anomaly_events schema에 evidence 컬럼 추가 필요 (마이그레이션 1개).
Slice E (ETA)        ─┤  pure compute.
Slice F (sandbox)    ─┤  Settings reader 재사용.
Slice G (cost split) ─┘
Slice H (cross-pane corr) ── Slice D 직후가 자연스러움.
Slice I (audit chip) ── 가장 가벼움.
```

권장 순서: **A → C → B → E → D → H → G → F → I**.
- A: 즉시 한눈 운영 회복.
- C: 운영자 incentive 회복 (행동의 payoff 보여주기).
- B: prescriptive 보강.
- E: 사전 경보로 spec §9.2 F의 정신 회복.
- D, H: anomaly narrative.
- G: cost 책임 추적.
- F, I: spec §11.2 끝맺음.

## 4.11 비목표 (이번 라운드에서 안 할 것)

- 새 modal 추가하지 않음.
- 새 detector 추가하지 않음 (8개로 충분).
- automatic actuation 추가하지 않음 (recommendation-first 유지).
- FX/테마/grid 등 cosmetic 추가 작업 안 함.
- mission ledger 구조 변경 안 함.

---

# 5. 검증 계획

## 5.1 각 슬라이스의 done_when

| Slice | done_when |
|---|---|
| A Now strip | 5 priority 분기 unit test 통과 + 빈 reports 시 healthy 표시 |
| B Next-best-action | 4 시나리오 (pending strong rec / no rec / multi rec / data unavailable) test |
| C payoff | 4 시나리오 (saved / neutral / regressed / no data) test + minimum sample/threshold 둘 다 honor |
| D evidence expansion | inline expand toggle test + history persistence migration test |
| E ETA | 6 시나리오 + R² confidence threshold test |
| F sandbox strip | 3 provider × 2 state matrix test |
| G cost breakdown | 3 grouping (pane/model/situation) × 2 (data/no-data) test |
| H cross-pane correlation | 2-pane same-minute fixture + spurious-correlation negative test |
| I audit chip | severity gating test + recency window test |

## 5.2 회귀 방어

- 1445 lib + 65 integration test 그대로 통과 유지.
- fmt + clippy clean.
- 새 surface마다 wrap test (좁은 viewport에서 truncation 없음).
- 새 lifecycle 컬럼은 backward-compat migration (기존 row default 처리).

## 5.3 운영자 피드백 루프

- 슬라이스 A/B/C는 1주 내부 dogfooding (chquan 본인 사용).
- 슬라이스 E는 ETA 정확도 measurement: 실제 도달 시점 vs ETA 예측의 오차 분포를 audit_events 또는 신규 metric `eta_prediction_error`로 1주 수집 후 평가.

---

# 6. 위험과 완화

| 위험 | 완화 |
|---|---|
| Now strip이 alerts와 시각적으로 충돌 | chip 형태, dimmer 색, 1-line 제약 |
| ETA 오류로 운영자 신뢰도 하락 | sample count + slope confidence + “[Est]” 라벨 강제 |
| 슬라이스 D에 evidence 스키마 추가 — migration 위험 | additive 컬럼 + NULL default + read-side fallback (현재 reason 컬럼만 사용) |
| “더 보여주기”가 결국 surface area 증가 | 새 modal 금지, 기존 surface 안에서만 추가 |
| operator workflow 학습 부담 | help modal에 “workflow primer” section 1개 추가 (Slice B와 같이 가도 됨) |
| automatic actuation으로의 미끄러짐 | spec §9.1 Layer 5 “destructive 자동화 금지” 원칙 그대로 유지, 모든 추가 surface는 read-only |

---

# 7. 한 줄 결론

> Qmonster v2.0.0은 **수집·저장·정직성**에서 spec을 초과 달성했지만, **표출의 합성성(synthesis)** — 모은 데이터를 한 화면 위에서 인과적으로 묶어주는 능력 — 에서 spec의 “alert-first / 한눈 운영” 정신과 거리가 생겼다. 다음 라운드의 과제는 *데이터 더 모으기*가 아니라 *지금 모은 데이터로 운영자에게 “지금 한 가지만 하면 된다”를 한 줄로 알려주는 것*이다.

---

# 8. 부록 A — 코드 위치 참조

| 화면 | 코드 |
|---|---|
| Alerts panel | `src/ui/alerts.rs` |
| Panes panel | `src/ui/panels.rs` |
| Dashboard layout | `src/ui/dashboard.rs` `render_dashboard` |
| Footer (★p ★y) | `src/ui/dashboard.rs` `render_footer` + `pending_proposal_summary` |
| Metrics overlay (`m`) | `src/ui/metrics.rs` `render_metrics_lines` |
| Insights overlay (`i`) | `src/ui/insights.rs` + `src/insights_report.rs` |
| Anomaly overlay (`n`) | `src/ui/anomaly_overlay.rs` |
| Settings overlay (`S`) | `src/ui/settings.rs` |
| Provider Setup (`P`) | `src/ui/provider_setup.rs` |
| Engine | `src/policy/engine.rs` `Engine::evaluate` |
| Gates | `src/policy/gates.rs` |
| Rules | `src/policy/rules/{advisories,profiles,auto_memory,agent_memory,cache,profile_switch,reset,idle,concurrent,cost_budget,identity_drift,anomaly}.rs` |
| Adapters | `src/adapters/{claude,codex,codex_app_server,gemini,common,runtime,agent_memory,process_memory,claude_sidefile,qmonster}.rs` |
| Audit sink | `src/store/audit.rs` |
| Token sink | `src/store/token_usage.rs` |
| Cost sink | `src/store/cost_usage.rs` |
| Anomaly sinks | `src/store/{anomaly_history,anomaly_sink}.rs` |
| Lifecycle | `src/store/recommendation_lifecycle.rs` |
| Insights store | `src/store/insights.rs` |

---

# 9. 부록 B — 평가 시 발견한 spec-impl 일치 매트릭스

| spec 항목 | 위치 | 상태 |
|---|---|---|
| §0 두 축 (alert-first + token opt by architecture) | 전반 | ◎ |
| §4.1 4 pane 운영 단위 | tmux 설정 + identity layer | ◎ |
| §4.2 provider+instance+role+pane_id identity | `src/domain/identity.rs` | ◎ |
| §6 .docs / docs/ai / .mission 분리 | repo layout | ◎ |
| §9 토큰 최적화 5층 | policy + store + ui | ○ (Layer 5 actuation 의도적 보수) |
| §9.2 시나리오 A–F | 14 rule | ◎ (G/H 추가) |
| §9.3 /compact, /memory, /clear, cache 운영 | snapshots + cache rule + manual 권고 | ○ |
| §10 provider profile | profiles rule + format_profile_lines | ◎ |
| §11.1 alert-first | alerts panel | ○ (Now strip 추가 권장) |
| §11.2 정보 패널 14 항목 | 카드 + metric badges | △ (sandbox/audit strip 부재) |
| §11.3 저채도 팔레트 | theme.rs Light/HighContrast | ◎ |
| §11.4 official vs estimated | SourceKind label | ◎ (구현이 spec을 초과) |
| §12 보안·감사 9 이벤트 | audit_events table | ○ (운영자 surface 부족) |
| §12.3 finding severity 5단계 | Severity enum | ◎ |
| §13 config vs runtime state 분리 | config.rs vs SignalSet | ◎ |
| §15 phase 1–5 로드맵 | Phase 1–7 + v1.36–v2.0 polish bundles | ◎ |
| §16 시작 순서 | bootstrap.sh 보존 | ◎ |
| §17 파일명 규칙 | `.docs/{model}/Qmonster-vX.Y.Z-DATE-…-rN.md` | ◎ |

종합: 8 ◎ + 5 ○ + 2 △ + 0 ▼. spec 자체는 압도적으로 잘 따르고 있고, 남은 △는 “표출의 합성성”에 모여 있음.

---

_(end of r1 — codex/gemini 교차 검증 후 r2로 갱신)_
