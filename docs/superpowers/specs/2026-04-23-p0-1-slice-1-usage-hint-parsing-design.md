# P0-1 Slice 1 — Provider usage hint parsing (v1.11.0)

- Status: **Approved** (2026-04-23)
- Scope: observe-first → first population of token_count / context_pressure / model_name / cost_usd on real pane tails
- Precedes: P0-1 Slice 2 (SignalSet `git_branch` / `reasoning_effort` / Claude model), P0-1 Slice 3 (Gemini)
- Version target: `v1.11.0`

## 1. Motivation

v1.10.9까지 `SignalSet` 안에 `context_pressure`, `token_count`, `cost_usd` 필드가 존재하고 UI 렌더 (`src/ui/panels.rs::metric_row`, `::metric_badge_line`)도 값이 있으면 출력하도록 구현되어 있지만, **adapter가 거의 아무것도 채우지 않는 상태**다 (Claude adapter의 Heuristic `context 88%` 휴리스틱 하나뿐). 운영자가 TUI를 열면 실제로 다음 질문에 대한 답을 얻을 수 없다:

- "이 pane에서 토큰을 얼마나 썼나?" → 화면에 숫자 없음
- "이 Codex 세션이 어느 모델로 돌고 있나?" → 화면에 없음
- "이 세션의 context가 얼마나 찼나?" → Claude heuristic 추측만

실세션 아카이브(`~/.qmonster/archive/`) 조사 결과, **Codex CLI는 한 줄짜리 상태 라인에 거의 모든 필요한 데이터를 ProviderOfficial 수준으로 노출**한다. Claude Code는 작업 상태 라인에서 출력 토큰을 감지 가능한 형태로 emit한다. 이를 파싱해서 `SignalSet`을 채우는 것이 이 슬라이스의 목표다.

## 2. Empirical evidence (실세션 샘플)

### 2.1 Codex CLI status bar (`~/.qmonster/archive/2026-04-23/_65/...log`)

```
Context 73% left · ~/Qmonster · gpt-5.4 · Qmonster · main · Context 27% used · 5h 98% · weekly 99% · 0.122.0 · 258K window · 1.53M used · 1.51M in · 20.4K out · 019db0ff-cf26-79d0-84ca-8be9b63f1c39 · gp...
```

파싱 가능한 값:

- `Context 27% used` → context_pressure (ProviderOfficial)
- `gpt-5.4` → model_name
- `main` → git branch (Slice 2)
- `5h 98%` / `weekly 99%` → quota (Slice 2)
- `258K window` → context window size
- `1.53M used` → 총 토큰
- `1.51M in` / `20.4K out` → 입력/출력 분리 (cost 계산용)
- `0.122.0` → CLI 버전

### 2.2 Claude Code working line (panes `_59`, `_63`)

```
✻ Frolicking… (running stop hooks… 0/3 · 2m 24s · ↓ 4.3k tokens)
✶ Exploring adapter parsing surface… (1m 34s · ↓ 4.1k tokens · thought for 11s)
✽ Exploring adapter parsing surface… (3m 0s · ↓ 8.6k tokens)
  ⎿  Done (27 tool uses · 95.1k tokens · 1m 21s)
```

파싱 가능한 값:

- `↓ 4.3k tokens` → 출력 토큰 누적 (세션 기준)
- `Done (… · 95.1k tokens · …)` → subagent 완료 시 더 정확한 누적 요약

파싱 불가 (Slice 1 out):

- 모델명: 작업 라인·배너 어디에도 노출 안 됨 (`Claude Code v2.1.118` CLI 버전만)
- 입력 토큰: 노출 없음 → cost_usd 계산 불가

### 2.3 Gemini CLI

현 아카이브(2026-04-23)에 운영 샘플 없음. Slice 3로 분리.

## 3. Scope

### 3.1 IN (Slice 1)

| 변경                                                             | 위치                                                   |
| ---------------------------------------------------------------- | ------------------------------------------------------ |
| `SignalSet.model_name: Option<MetricValue<String>>` 신규 필드    | `src/domain/signal.rs`                                 |
| `PricingTable` 모듈 + `config/pricing.example.toml` 로더         | `src/policy/pricing.rs`, `config/pricing.example.toml` |
| `ProviderParser::parse` 시그니처 3번째 인자 `&PricingTable` 추가 | `src/adapters/mod.rs`                                  |
| Codex 상태 라인 파서 + cost 계산 inline                          | `src/adapters/codex.rs`                                |
| Claude 작업-라인 출력 토큰 파서                                  | `src/adapters/claude.rs`                               |
| UI: model badge 렌더 추가 + count-suffix (K/M) 포맷터            | `src/ui/panels.rs`, `src/ui/labels.rs` 검토            |
| 단위 + 통합 테스트                                               | 각 모듈 `mod tests`, `tests/event_loop_integration.rs` |

### 3.2 OUT (deferred or permanent non-goal)

| 항목                                                             | 대상              | 이유                                                                              |
| ---------------------------------------------------------------- | ----------------- | --------------------------------------------------------------------------------- |
| Claude `cost_usd`                                                | Slice 2+          | 입력 토큰 미노출. 출력만으로 계산 시 lower-bound → 정직성 위반                    |
| Claude `model_name`                                              | Slice 2+          | tail·배너 어디에도 감지 가능한 형태 없음. settings.json 읽기 등 별도 surface 필요 |
| Gemini 전체                                                      | Slice 3           | 실세션 샘플 부재                                                                  |
| `git_branch`, `reasoning_effort`, `worktree_path` SignalSet 필드 | Slice 2           | 이번 슬라이스 schema 팽창 억제                                                    |
| 자동 가격 fetch (`gh api`, 공식 pricing API 조회)                | **영구 non-goal** | observe-first 원칙 위반                                                           |
| pricing 추정치를 Qmonster가 emit                                 | **영구 non-goal** | operator가 직접 입력. Qmonster는 빈 placeholder만 배포                            |

## 4. Design

### 4.1 `SignalSet` schema change

`src/domain/signal.rs`:

```rust
pub struct SignalSet {
    // ... 기존 필드 ...
    pub context_pressure: Option<MetricValue<f32>>,
    pub token_count: Option<MetricValue<u64>>,
    pub cost_usd: Option<MetricValue<f64>>,
    pub model_name: Option<MetricValue<String>>,  // 신규
}
```

**영향 분석**:

- `SignalSet`은 `Recommendation`과 구조적으로 분리되어 있어 Recommendation 생성자 25+곳 수정 **불필요**.
- 구성 위치 대부분이 `Default::default()` 또는 builder → 자동으로 `None`.
- 영향 받는 곳: struct-literal `SignalSet { ... }` 직접 생성 사이트. 구현 전 `rg "SignalSet \{" src/ tests/`로 예상 3~5곳 확인.

### 4.2 `PricingTable` 모듈

`src/policy/pricing.rs` 신규:

```rust
use std::collections::HashMap;
use crate::domain::identity::Provider;

pub struct PricingRates {
    pub input_per_1m: f64,
    pub output_per_1m: f64,
}

#[derive(Default)]
pub struct PricingTable {
    entries: HashMap<(Provider, String), PricingRates>,
}

impl PricingTable {
    pub fn load_from_toml(path: &Path) -> Result<Self, PricingError> { /* TOML 파싱 */ }
    pub fn empty() -> Self { Self::default() }
    pub fn lookup(&self, provider: Provider, model: &str) -> Option<&PricingRates> { ... }
}
```

**로딩 동작**:

- `QMONSTER_ROOT/config/pricing.toml` 존재 → 파싱 (실패 시 startup audit 기록 + empty table)
- 부재 → empty table (모든 cost 계산 skip, `cost_usd = None`)

`config/pricing.example.toml`:

```toml
# Qmonster pricing table (Estimated, operator-curated)
#
# Values are USD per 1 million tokens. Leave placeholders at 0.00 to skip cost
# estimation for a given (provider, model) pair -- Qmonster will render no
# "COST [Est]" badge for that combination.
#
# This table is ProjectCanonical. Qmonster does NOT fetch provider pricing
# pages. Refresh manually when prices change; the file is gitignored so each
# operator owns their own numbers. Cost estimates are for trend tracking,
# NOT for billing reconciliation.
#
# Last updated by operator: YYYY-MM-DD

[[entries]]
provider = "codex"
model = "gpt-5.4"
input_per_1m = 0.00    # TODO(operator): fill in
output_per_1m = 0.00

[[entries]]
provider = "claude"
model = "claude-sonnet-4-6"
input_per_1m = 0.00
output_per_1m = 0.00
```

파일은 `.gitignore`에 `config/pricing.toml` 추가 (example만 커밋, operator 실제 숫자는 local).

### 4.3 `ProviderParser` trait 확장

`src/adapters/mod.rs`:

```rust
pub trait ProviderParser {
    fn parse(
        &self,
        identity: &ResolvedIdentity,
        tail: &str,
        pricing: &PricingTable,
    ) -> SignalSet;
}

pub fn parse_for(
    identity: &ResolvedIdentity,
    tail: &str,
    pricing: &PricingTable,
) -> SignalSet {
    match identity.identity.provider {
        Provider::Claude => claude::ClaudeAdapter.parse(identity, tail, pricing),
        Provider::Codex => codex::CodexAdapter.parse(identity, tail, pricing),
        Provider::Gemini => gemini::GeminiAdapter.parse(identity, tail, pricing),
        Provider::Qmonster => qmonster::QmonsterAdapter.parse(identity, tail, pricing),
        Provider::Unknown => common::parse_common_signals(tail),
    }
}
```

Claude / Gemini / Qmonster / Unknown 구현은 `_pricing: &PricingTable`로 받고 무시 (Slice 1에선 Codex만 사용).

**Caller 업데이트** (`src/app/event_loop.rs`): 앱 시작 시 `PricingTable::load_from_toml(root.join("config/pricing.toml"))` 호출 → `Arc<PricingTable>` 또는 `&PricingTable`을 이벤트 루프에 주입 → `parse_for` 호출 시 전달.

### 4.4 Codex adapter

`src/adapters/codex.rs`:

```rust
pub struct CodexAdapter;

struct CodexStatus {
    context_pct: u8,       // "27" from "Context 27% used"
    total_tokens: u64,     // 1_530_000 from "1.53M used"
    input_tokens: u64,     // 1_510_000 from "1.51M in"
    output_tokens: u64,    // 20_400 from "20.4K out"
    model: String,         // "gpt-5.4"
}

fn parse_codex_status_line(tail: &str) -> Option<CodexStatus> {
    // bottom-up 스캔, 첫 매치 사용
    for line in tail.lines().rev() {
        if !(line.contains("Context") && line.contains("% used") && line.contains(" · ")) {
            continue;
        }
        let tokens: Vec<&str> = line.split(" · ").map(str::trim).collect();
        // 각 토큰을 개별 패턴으로 매치 (context_pct, input/output/total tokens, model)
        // ...
        return Some(CodexStatus { ... });
    }
    None
}

impl ProviderParser for CodexAdapter {
    fn parse(
        &self,
        _identity: &ResolvedIdentity,
        tail: &str,
        pricing: &PricingTable,
    ) -> SignalSet {
        let common = parse_common_signals(tail);
        let Some(status) = parse_codex_status_line(tail) else {
            return common;
        };

        let cost_usd = pricing
            .lookup(Provider::Codex, &status.model)
            .filter(|rates| rates.input_per_1m > 0.0 || rates.output_per_1m > 0.0)
            .map(|rates| {
                let cost = (status.input_tokens as f64 * rates.input_per_1m
                    + status.output_tokens as f64 * rates.output_per_1m)
                    / 1_000_000.0;
                MetricValue::new(cost, SourceKind::Estimated)
                    .with_confidence(0.7)
                    .with_provider(Provider::Codex)
            });

        SignalSet {
            context_pressure: Some(
                MetricValue::new(status.context_pct as f32 / 100.0, SourceKind::ProviderOfficial)
                    .with_confidence(0.95)
                    .with_provider(Provider::Codex),
            ),
            token_count: Some(
                MetricValue::new(status.total_tokens, SourceKind::ProviderOfficial)
                    .with_confidence(0.95)
                    .with_provider(Provider::Codex),
            ),
            model_name: Some(
                MetricValue::new(status.model, SourceKind::ProviderOfficial)
                    .with_confidence(0.95)
                    .with_provider(Provider::Codex),
            ),
            cost_usd,
            ..common
        }
    }
}
```

**토큰 suffix 파싱**: 공용 helper `parse_count_with_suffix("1.53M") -> Option<u64>` — `K/k` → ×1_000, `M/m` → ×1_000_000, 없으면 그대로. **위치**: `src/adapters/common.rs`에 배치 (adapter layer 내부 도구이며 Claude adapter도 같은 함수를 공유하므로 common이 맞음).

**상태 라인 disambiguation**: Codex는 (a) `/status` 명령 출력의 bordered box 내부에도 "Context" + "· " 패턴이 있고 (b) pane 하단의 상태 바에도 있음. bottom-up 스캔은 **가장 최신 프레임 = 하단 상태 바**를 먼저 매치하도록 설계됨. `/status` 출력은 명령 실행 시점의 스냅샷이라 stale일 수 있어 의도적으로 뒤에 둠.

### 4.5 Claude adapter

`src/adapters/claude.rs`:

```rust
fn parse_claude_output_tokens(tail: &str) -> Option<u64> {
    // 우선순위 1: Done (… · N tokens · …) — subagent 완료 요약
    // 우선순위 2: ↓ N tokens — 현재 작업 라인
    for line in tail.lines().rev() {
        if let Some(captures) = DONE_LINE_RE.captures(line) {
            return parse_count_with_suffix(captures.get(1)?.as_str());
        }
    }
    for line in tail.lines().rev() {
        if let Some(captures) = WORKING_LINE_RE.captures(line) {
            return parse_count_with_suffix(captures.get(1)?.as_str());
        }
    }
    None
}

impl ProviderParser for ClaudeAdapter {
    fn parse(
        &self,
        _identity: &ResolvedIdentity,
        tail: &str,
        _pricing: &PricingTable,
    ) -> SignalSet {
        let mut common = parse_common_signals(tail);

        // 기존 동작 유지: Claude-specific "claude context N%" 패턴이 더
        // 구체적이므로 common의 heuristic을 override 한다 (둘 다 매치되면
        // Claude-specific이 우선). Estimated 라벨은 그대로 — Claude tail에
        // 공식 % 표기는 없음.
        if let Some(pct) = parse_context_percent_claude(tail) {
            common.context_pressure = Some(
                MetricValue::new(pct / 100.0, SourceKind::Estimated)
                    .with_confidence(0.6)
                    .with_provider(Provider::Claude),
            );
        }

        common.token_count = parse_claude_output_tokens(tail).map(|n| {
            MetricValue::new(n, SourceKind::ProviderOfficial)
                .with_confidence(0.85)
                .with_provider(Provider::Claude)
        });

        common
    }
}
```

Claude `model_name`과 `cost_usd`는 명시적으로 `None` 유지 (Slice 1 정직성).

**Confidence 차이 이유**:

- Codex 0.95: 공식 상태 바는 provider가 안정적으로 매 프레임 렌더
- Claude 0.85: 작업 중에만 나타남. 완료 후 사라질 수 있고 (세션 종료 전 마지막 프레임이 아닐 수 있음), 출력 토큰만 노출

### 4.6 UI changes

`src/ui/panels.rs`:

**`metric_row` 추가**:

```rust
if let Some(m) = s.model_name.as_ref() {
    parts.push(format!("model {} [{}]", m.value, source_kind_label(m.source_kind)));
}
```

**`metric_badge_line` 추가**:

```rust
if let Some(m) = signals.model_name.as_ref() {
    spans.push(Span::styled(
        format!(" MODEL {} [{}] ", m.value, source_kind_label(m.source_kind)),
        theme::label_style(),
    ));
}
```

**Count-suffix 포맷터** (신규 helper, `src/ui/labels.rs`에 배치 — 기존 `source_kind_label` 같은 UI formatting helper들과 coherent):

```rust
pub fn format_count_with_suffix(n: u64) -> String {
    if n >= 1_000_000 {
        format!("{:.2}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}K", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}
```

`token_count` 렌더 포맷을 기존 `format!("tokens {}", m.value)` → `format!("tokens {}", format_count_with_suffix(m.value))`로 바꿔서 `1530000` → `1.53M` 표기.

## 5. Testing

### 5.1 Unit tests

**`src/policy/pricing.rs`**:

- `pricing_table_loads_entries_from_toml`
- `pricing_table_lookup_returns_none_for_missing_entry`
- `pricing_table_empty_has_no_entries`
- `pricing_table_treats_zero_rate_entries_as_unset` — **포함**. zero rate = "operator가 아직 안 채웠음"으로 간주하여 `cost_usd = None`이 되는 계약을 lock. §4.4 Codex `.filter(|r| r.input_per_1m > 0.0 || r.output_per_1m > 0.0)` 동작과 짝.

**`src/adapters/codex.rs`**:

- `codex_adapter_extracts_four_metrics_from_status_line_with_pricing` — 실세션 fixture (redacted), 4필드 assert + ProviderOfficial/Estimated 라벨 분리 assert
- `codex_adapter_leaves_cost_none_when_pricing_table_empty` — 3필드는 populate, cost만 None
- `codex_adapter_falls_back_to_common_when_status_line_absent` — regression
- `codex_adapter_parses_suffix_k_and_m_correctly` — `258K`, `1.53M`, `20.4K` 변환 검증

**`src/adapters/claude.rs`**:

- `claude_adapter_extracts_output_tokens_from_working_line` — `↓ 4.3k tokens` → 4300
- `claude_adapter_prefers_subagent_done_line_over_working_line` — 두 패턴 공존 시 Done 우선
- `claude_adapter_returns_none_token_count_when_no_marker` — regression
- `claude_adapter_never_populates_model_name_or_cost_in_slice_1` — 정직성 regression

**`src/ui/panels.rs` 또는 format helper**:

- `metric_row_renders_model_name_line_when_populated`
- `metric_badge_line_renders_model_badge_when_populated`
- `format_count_with_suffix_handles_k_m_boundaries` — `999 → "999"`, `1000 → "1.0K"`, `999_999 → "1000.0K"`, `1_000_000 → "1.00M"`

### 5.2 Integration tests

`tests/event_loop_integration.rs`:

- `codex_status_line_end_to_end_with_pricing_populates_four_metrics`
- `codex_status_line_end_to_end_without_pricing_populates_three_metrics`

### 5.3 Fixture PII 처리

실세션 아카이브 라인에서 다음은 반드시 `<redacted>` 치환:

- session UUID (`019db0ff-cf26-...`)
- 계정 이메일 (Codex `/status` 출력의 `Account: user@example.com`)
- 절대 경로에 사용자 홈 디렉토리가 있으면 `~/` 형태 유지 (이미 Codex 자체가 `~` 사용)

## 6. Version / commit plan

**Target version**: `v1.11.0` (minor bump — 새 관찰 surface + SignalSet schema 확장)

**Commits** (5개, TDD 기반):

| #   | 메시지                                                                                                        | 범위                                                                             |
| --- | ------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- |
| 1   | `policy(v1.11.0-1): add pricing table module + example TOML config`                                           | `src/policy/pricing.rs`, `config/pricing.example.toml`, `.gitignore`             |
| 2   | `domain(v1.11.0-2): add model_name field to SignalSet`                                                        | `src/domain/signal.rs` + 직접 struct-literal 사이트 업데이트                     |
| 3   | `adapters(v1.11.0-3): extend ProviderParser trait with pricing + codex status line parser + cost computation` | `src/adapters/mod.rs`, `src/adapters/codex.rs`, `src/app/event_loop.rs` (caller) |
| 4   | `adapters(v1.11.0-4): claude working-line output tokens`                                                      | `src/adapters/claude.rs`                                                         |
| 5   | `ui(v1.11.0-5): render model badge + count-suffix formatter`                                                  | `src/ui/panels.rs`, `src/ui/labels.rs` 또는 `src/util/format.rs`                 |

**Annotated tag**: `v1.11.0` at commit 5.

**상태 파일**: `.mission/CURRENT_STATE.md` Mission/Phase 줄 교체 (gitignored).

**예상 test 증가**: 280 → ~297 (+17 — pricing 4 / codex 4 / claude 4 / ui 3 / integration 2).

## 7. Review cycle

관찰 surface 변경 + SignalSet schema 확장 + `Estimated` cost 도입 → **Codex + Gemini confirm-archive 리뷰 필수**.

**Reviewer 예상 논점** (pre-emptive framing):

1. `Estimated` cost 라벨이 operator에게 충분히 분명한가? (UI에서 `[Est]` vs `[PO]` 시각 구분)
2. `pricing.toml` placeholder 0.00 → cost None 처리가 정직한가?
3. Codex `input_tokens`/`output_tokens`를 SignalSet에 노출하지 않은 결정 (adapter 내부 계산에만 사용) — schema minimalism 유지 vs 투명성
4. Claude `cost_usd = None` 정직성 regression test 유무

**Review 산출물 경로**:

- 내러티브: `.docs/{codex,gemini}/Qmonster-v0.4.0-2026-04-23-v1.11.0-review.md`
- 구조적 미러: `.mission/evals/Qmonster-v0.4.0-2026-04-23-v1.11.0-review.result.yaml`
- Confirm: `...-v1.11.0-confirm-review.md`

## 8. Acceptance criteria

- [ ] `cargo fmt --check` clean
- [ ] `cargo clippy --all-targets -- -D warnings` clean
- [ ] 전체 테스트 green (~297 expected)
- [ ] `config/pricing.example.toml` 존재 + `.gitignore`에 `config/pricing.toml` 추가
- [ ] 실세션 Codex pane의 TUI 패널에 `context 27% [PO]`, `tokens 1.53M [PO]`, `model gpt-5.4 [PO]` 표시 확인 (operator 육안 검증)
- [ ] `config/pricing.toml`이 operator-supplied 값으로 채워졌을 때 `COST $X.XX [Est]` 렌더 확인
- [ ] Claude pane의 TUI 패널에 `tokens 95.1K [PO]` 표시 + `cost` / `model` 배지 **미표시** 확인 (정직성)
- [ ] `.mission/CURRENT_STATE.md` v1.11.0 요약으로 갱신
- [ ] Codex + Gemini 둘 다 `approve` (혹은 remediation 통해 모든 finding close)

## 9. Risks / unknowns

1. **Codex 상태 라인 포맷 drift**: Codex CLI 0.122.0 기준. 향후 CLI 업데이트로 포맷이 변경될 수 있음. → `codex_adapter_falls_back_to_common_when_status_line_absent` 테스트가 regression 보호. 실제 포맷 변경 시 다음 라운드에서 파서 업데이트.
2. **Claude working-line 제거 가능성**: Claude Code가 향후 "quiet mode" 같은 옵션으로 작업 상태 라인을 숨기면 `token_count`가 `None`이 됨. → 운영자 토글이므로 Qmonster가 제어 불가. 문서화만.
3. **`pricing.toml` 가격 구식화**: operator가 모델 가격 변경을 놓치면 cost 추정이 부정확해짐. → example.toml 헤더에 "Refresh manually" 명시 + `Last updated: YYYY-MM-DD` 필드로 가시화.
4. **테스트 fixture가 실세션 아카이브와 drift**: 초기 fixture가 특정 시점의 pane 출력 기준. → 테스트 이름에 "Codex CLI 0.122.0" 버전 표기 + 시간 경과 후 fixture 업데이트 가능하도록 문서화.

## 10. References

- 실세션 아카이브: `~/.qmonster/archive/2026-04-23/_65/`, `_59/`, `_63/`
- 기존 MetricValue 사용 패턴: `src/domain/signal.rs:6-33`
- UI 렌더 함수: `src/ui/panels.rs:341-365` (`metric_row`), `:425-466` (`metric_badge_line`)
- SourceKind 분류: `src/domain/origin.rs`
- 유사 슬라이스 참고 (schema 확장 + 테스트 패턴): Phase 4 P4-1 `profile` 필드 추가 (`src/domain/recommendation.rs`, mission-history.yaml change_sequence 13)
