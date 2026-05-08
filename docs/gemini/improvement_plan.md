# Qmonster Implementation Evaluation & Improvement Plan (Gemini)

## 1. 구현 평가 (Evaluation against Original Goals)

**화면 및 정보의 의미 (UI & Information Value):**
- **평가:** Qmonster의 TUI 화면은 tmux 기반 환경에서 여러 AI 에이전트(Claude, Codex, Gemini 등)의 상태를 단일 뷰에서 모니터링할 수 있게 해줍니다. 단순한 텍스트 기반 인터페이스임에도 Metrics(m), Pending Actions(a), Anomaly Events(n), Token Insights(i) 등의 오버레이를 통해 운영자에게 매우 가치 있고 액션 가능한(actionable) 정보를 제공합니다.

**분석, 통계, 시각화의 의미 (Meaningfulness of Analytics & Visualizations):**
- **평가:** `TOKENS ▁▂▃▄▅▆▇█` 와 같은 스파크라인, 상태에 따른 색상 코딩, 캐시 적중률 배지 등은 단순하지만 직관적입니다. 터미널이라는 제약 속에서도 토큰 사용량, 비용, 메모리 증가 등의 시계열 데이터를 시각적으로 훌륭하게 압축하여 전달합니다.

**데이터 수집 및 표출 정확성 (Data Collection & Display):**
- **평가:** SQLite 기반의 지속성 스토어(`token_usage_samples`, `cost_usage_events`, `anomaly_events`)를 통해 각 폴링 주기마다 데이터를 정확히 수집합니다. Provider의 공식 출력(Claude sidefile, Codex CLI 상태 등)을 직접 파싱하여 `SourceKind::ProviderOfficial` 형태로 관리하므로 신뢰성이 매우 높습니다.

**토큰 최적화의 실효성 (Helpfulness of Token Optimization):**
- **평가:** 토큰 최적화가 단순 부가 기능이 아닌 '아키텍처 5계층'으로 설계되어 강력합니다. 입력/출력 토큰과 캐시 적중률을 계산하여 `/compact` 등의 아카이브 액션을 추천하는 기능은, 실제 API 비용을 통제하고 컨텍스트 초과로 인한 에이전트 오작동을 예방하는 데 핵심적인 역할을 합니다.

**이상 징후 및 인사이트 (Anomaly & Insights):**
- **평가:** 8가지 Anomaly 종류(IdentityDrift, CostSlope, TokenSlope, MemoryGrowth, SubagentSideEffect 등)를 통해 에이전트 폭주나 리소스 누수를 조기에 감지하는 로직이 훌륭합니다. 단순 경고에 그치지 않고 이를 Recommendation으로 승격시키는 체계는 무인 모니터링의 실효성을 극대화합니다.

---

## 2. 초기 개선 계획 (Initial Improvement Plan)

1. **시각화 해상도 및 상호작용 강화 (Enhanced Visual Resolution & Interaction)**
   - **계획:** 특정 Metric이나 Anomaly 선택 시, 에이전트가 남긴 원본 로그 조각을 Diff 형태로 즉각 보여주는 'Deep Dive' 뷰를 도입하여 문제 파악 시간을 단축합니다.

2. **교차 에이전트 컨텍스트 분석 (Cross-Agent Context Analysis)**
   - **계획:** 현재 격리된 에이전트별 분석을 확장하여, 동일 프로젝트 내 에이전트 간 중복 프롬프트 및 캐시 낭비를 식별하는 'Global Token Duplicate Detector' 로직을 추가합니다.

3. **비용/토큰 임계치 기반 자동 반응형 프로필 스위칭 (Cost-Reactive Profile Switching)**
   - **계획:** 80% / 100% 경고를 넘어, 예산 소진 속도(Cost/Token Slope)가 가파를 경우 자동으로 저비용 고압축 프로필(예: Haiku 모델)로 전환을 제안하거나 자동 실행하는 기능을 고도화합니다.

4. **대시보드 레이아웃 동적 최적화 (Dynamic Layout Optimization)**
   - **계획:** 터미널 해상도와 활성화된 오버레이(m, a, n, i) 개수에 따라 중요도가 낮은 정보를 자동으로 접어주는(Auto-collapse) 반응형 레이아웃 알고리즘을 도입하여 가독성을 높입니다.

---

## 3. 교차 검증 및 통합 개선 계획 (Cross-Validation & Updated Plan)

Claude와 Codex의 산출물(`docs/claude/Qmonster-v2.0.0-2026-05-08-claude-init-vs-impl-evaluation-r1.md`, `docs/codex/improvement_plan.md`)을 교차 검증한 결과, 데이터 수집 아키텍처의 깊이는 훌륭하나 **1) 데이터 식별의 취약성(Identity Conflict)** 과 **2) 표면적 파편화(UI Dispersion) 및 피드백 부재** 라는 치명적 약점이 확인되었습니다. 이를 반영하여 최종 개선 계획을 우선순위별로 재조정합니다.

### P0. 데이터 귀속 및 식별 안정화 (Data Attribution & Identity Stability) - *Codex 지적 수용*
- **문제점:** `node /usr/bin/gemini --yolo` 와 같은 wrapper 프로세스나 stale title로 인해, 수집된 공식 토큰/비용 데이터가 엉뚱한 Provider(예: Claude)에 귀속되어 전체 화면의 신뢰도가 무너지는 심각한 리스크가 발견됨.
- **해결책:** Identity Resolver가 descendant argv/exe 정보를 깊게 파악하도록 개선하고, Canonical Title과 실제 Command가 충돌할 경우 `identity conflict`로 표시하여 오염된 지표가 승격되는 것을 차단합니다.

### P1. "Now" 통합 스트립 및 Next Best Action 제공 - *Claude 지적 수용*
- **문제점:** 정보가 7개의 Modal(`m i n S P t ?`)로 분산되어 "한눈 운영(at-a-glance)"을 해치며, Insights 오버레이는 사후 통계일 뿐 운영자에게 "다음 행동"을 지시하지 않음.
- **해결책:** 대시보드 Alerts 상단에 가장 시급한 행동 하나만을 띄우는 `Now Strip` (예: "WAIT INPUT — Enter approve")을 도입하고, Insights 최상단에 Next-Best-Action을 prescriptive하게 제시합니다.

### P1. 토큰 최적화 ROI(Payoff) 루프 시각화 - *Claude/Codex 공통 지적 수용*
- **문제점:** 운영자가 `/compact` 등 최적화 권고를 수락하더라도, 그로 인해 얼마의 토큰/비용이 절감되었는지 직관적으로 확인불가.
- **해결책:** 액션 완료 후 이전 상태와의 차이(예: "saved ~8K input tokens, $0.03")를 계산하여 Payoff를 한 줄로 시각화합니다. 최적화 도구로서의 체감 가치를 입증하는 핵심 고리입니다.

### P2. 이상 징후 인과 관계(Narrative) 및 예상 ETA 제공 - *Claude/Codex 공통 지적 수용*
- **문제점:** Anomaly가 단일 행으로만 표시되어 "왜 발생했는지(Evidence)"를 알 수 없고, 비용이나 메모리 경고가 임계치에 도달한 "사후"에만 경고됨.
- **해결책:** Anomaly 행 확장 시 관련된 지표의 before/after 등 Evidence를 서브 행으로 보여주고, 현재 기울기(Slope)를 바탕으로 임계치 도달 예상 시간(ETA chip)을 사전에 제공하여 선제적 조치를 유도합니다.

### P3. 동적 레이아웃 최적화 및 에이전트 간 중복 분석 - *Gemini 제안 유지*
- **해결책:** 터미널 제약을 넘기 위해 덜 중요한 정보를 접는 반응형 렌더링을 적용하고, 장기 과제로 동일 프로젝트 내 서브 에이전트 간의 컨텍스트 낭비를 탐지하는 글로벌 중복 탐지기(Global Token Duplicate Detector)를 구축합니다.
