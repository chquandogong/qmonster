use crate::app::config::HelpLanguage;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HelpTopic {
    AlertBulkHide,
    AlertHeader,
    AlertDismiss,
    AlertSummary,
    AlertDetail,
    AlertCopy,
    PaneHeader,
    PaneState,
    PanePath,
    PaneCommand,
    PaneStatus,
    PaneSignals,
    PaneMetrics,
    PaneTokens,
    PaneRuntime,
    PaneRecommendation,
    /// v1.58.0: Alerts/Panes split divider — drag-to-resize +
    /// HANGUL/IME indicator banner explanation.
    DashboardDivider,
    /// v1.58.0: bottom-row footer key cluster — pending action chips
    /// + the ` · `-separated keybinding reference.
    DashboardFooter,
    /// v1.58.0: bottom-right version badge — click to open Git overlay.
    DashboardVersionBadge,
    /// Topmost `Now` strip: one-line current-priority summary.
    DashboardNowStrip,
    /// Footer `★p` chip: pending prompt-send proposal count.
    DashboardFooterProposalChip,
    /// Footer `★y` chip: copyable suggested-command alert count.
    DashboardFooterCopyChip,
    /// Footer `★a` chip: recent audit severity indicator.
    DashboardFooterAuditChip,
}

pub fn language_label(language: HelpLanguage) -> &'static str {
    match language {
        HelpLanguage::Ko => "KO",
        HelpLanguage::En => "EN",
    }
}

pub fn help_lines(topic: HelpTopic, language: HelpLanguage) -> &'static [&'static str] {
    match language {
        HelpLanguage::Ko => ko_lines(topic),
        HelpLanguage::En => en_lines(topic),
    }
}

fn ko_lines(topic: HelpTopic) -> &'static [&'static str] {
    match topic {
        HelpTopic::AlertBulkHide => &[
            "bulk hide: 심각도별 알림을 한 번에 숨기거나 되돌립니다.",
            "Risk/Warning/Concern/Good 칩을 클릭하면 해당 등급의 실행 가능한 알림이 토글됩니다.",
            "자동 숨김 예정인 항목은 undo 상태로 다시 복구할 수 있습니다.",
        ],
        HelpTopic::AlertHeader => &[
            "헤더: 알림 발생 시각, NEW 여부, 심각도, 제목을 한 줄에 모읍니다.",
            "제목은 Recommendation/Checkpoint/FLOW와 관련 pane ID 또는 흐름 이름을 보여줍니다.",
            "FLOW는 같은 대응 경로를 공유하는 관련 alert를 하나로 묶은 행입니다.",
            "★y 칩은 바로 실행 가능한 추천 명령이 있음을 뜻합니다.",
        ],
        HelpTopic::AlertDismiss => &[
            "dismiss: 이 알림을 목록에서 숨길지 또는 숨김 예약을 되돌릴지 제어합니다.",
            "[ ] 상태는 click/Enter/Space로 hide, [x] 상태는 undo를 의미합니다.",
            "auto-hide 카운트다운은 숨김 예약이 진행 중임을 나타냅니다.",
        ],
        HelpTopic::AlertSummary => &[
            "summary: 알림이 발생한 핵심 원인이나 헤드라인입니다.",
            "예: 입력 대기, cross-pane 파일 충돌, 비용/토큰 압박 같은 판단 이유가 표시됩니다.",
        ],
        HelpTopic::AlertDetail => &[
            "details: summary 아래의 next/run/anchor/others 또는 FLOW rail 같은 추가 정보입니다.",
            "FLOW rail은 원인 신호 -> 후속 근거 -> 선행 조치 -> 실행 명령을 순서대로 보여줍니다.",
            "included 행은 FLOW가 묶은 원본 alert 근거입니다.",
        ],
        HelpTopic::AlertCopy => &[
            "copy: 선택된 알림에 실행 가능한 suggested_command가 있을 때만 나타나는 힌트입니다.",
            "알림에 포커스가 있을 때 y를 누르면 표시된 shell/slash 명령을 클립보드로 복사합니다.",
        ],
        HelpTopic::PaneHeader => &[
            "pane 헤더: tmux session/window, provider, role, CLI version, pane ID를 요약합니다.",
            "Qmonster pane은 CLI version 표시 대상에서 제외됩니다.",
            "★p 칩은 이 pane에 수락/거절 가능한 prompt-send 제안이 있음을 뜻합니다.",
        ],
        HelpTopic::PaneState => &[
            "state: IDLE DONE, WAIT INPUT, WAIT APPROVAL, USAGE LIMIT, IDLE STALE 같은 대기 상태입니다.",
            "상태 배지 옆 시간은 그 상태에 들어간 뒤 흐른 시간입니다.",
            "STATE CHANGED/ACTIVE는 최근 상태 전환을 강조합니다.",
        ],
        HelpTopic::PanePath => &[
            "path: 해당 pane의 현재 작업 디렉토리(CWD)입니다.",
            "경로가 길면 카드 폭에 맞춰 축약되며, 알 수 없으면 unknown path로 표시됩니다.",
        ],
        HelpTopic::PaneCommand => &[
            "cmd: 해당 pane에서 현재 실행 중이거나 마지막으로 보인 명령입니다.",
            "긴 명령은 카드 폭에 맞춰 줄바꿈됩니다.",
        ],
        HelpTopic::PaneStatus => &[
            "status: Qmonster가 pane의 provider/role을 얼마나 확실히 식별했는지 보여줍니다.",
            "high/medium/low/conflict confidence는 identity resolver의 신뢰도입니다.",
            "conflict는 title과 command가 서로 다른 provider를 가리켜 provider별 metric을 억제했다는 뜻입니다.",
        ],
        HelpTopic::PaneSignals => &[
            "signals: 터미널 출력과 동작 패턴에서 감지한 특이사항입니다.",
            "waiting for input, approval needed, log storm, repeated output, error hint 등이 표시됩니다.",
        ],
        HelpTopic::PaneMetrics => &[
            "metrics: 비용, context/quota 압박, reset ETA, model, branch/worktree 같은 런타임 지표입니다.",
            "COST는 금액에 따라 Good/Concern/Warning/Risk 색으로 변합니다.",
            "각 칩의 [Official]/[Estimated] 표기는 데이터 출처를 뜻합니다.",
        ],
        HelpTopic::PaneTokens => &[
            "tokens/cache io: 입력/출력 토큰과 prompt cache 생성/읽기 통계입니다.",
            "선택된 pane에서는 최근 토큰 변화 스파크라인과 token io가 함께 표시됩니다.",
        ],
        HelpTopic::PaneRuntime => &[
            "runtime facts: 세션, 권한 모드, sandbox, allowed dir, transcript, loaded tool/skill/plugin 정보입니다.",
            "session/loaded 행은 에이전트가 어떤 로그와 확장 기능으로 실행 중인지 추적하는 데 씁니다.",
        ],
        HelpTopic::PaneRecommendation => &[
            "recommendations: Qmonster가 발견한 문제, 제안, 다음 조치를 pane 카드 안에 요약합니다.",
            "Risk/Warning/Concern/Good/Safe 라벨 뒤에 이유와 detail/next/run이 붙습니다.",
        ],
        HelpTopic::DashboardDivider => &[
            "Alerts/Panes 분할 바: 드래그해 두 영역의 높이 비율을 조절합니다.",
            "키보드: `[`/`]` Alerts 줄이기/키우기, `/` 비율 순환, `=` 기본값 복원.",
            "비ASCII 알파벳 입력 시(한글/카타카나 등) `⚠ HANGUL/IME ACTIVE` 배너로 바뀌고 첫 활성화 시 BEL이 한 번 울립니다 (v1.51.0+, 3초 TTL).",
        ],
        HelpTopic::DashboardFooter => &[
            "Footer: 현재 focus, Alerts/Panes split 비율, 보류 중인 액션/감사 카운터(★p / ★y / ★a), 그리고 주요 키바인딩 클러스터를 한 줄에 모읍니다.",
            "★p는 수락 가능한 prompt-send 제안 수, ★y는 복사 가능한 suggested_command 알림 수, ★a는 최근 audit 심각도입니다.",
            "키 클러스터는 ` · `로 구분되며 모달이 열리면 풋터의 focus 표시가 `overlay`로 바뀝니다.",
        ],
        HelpTopic::DashboardVersionBadge => &[
            "버전 배지: 우측 하단에 `git describe --tags --always --dirty`로 박힌 빌드 버전이 표시됩니다.",
            "클릭하면 Git overlay가 열려 origin URL, 브랜치/HEAD, 변경 내역, Recent Commits, Contributors를 한 번에 볼 수 있습니다.",
            "v1.51.0부터 build.rs가 `.git/refs/tags`도 추적하므로 새 태그 부착 후 재빌드 시 자동으로 갱신됩니다.",
        ],
        HelpTopic::DashboardNowStrip => &[
            "Now row: 화면 맨 위에서 지금 가장 중요한 상태를 한 줄로 요약합니다.",
            "입력/승인 대기, Risk 추천, quota/cost 압박, 최근 anomaly, healthy 상태 순서로 우선순위를 정합니다.",
            "`p send`/`d dismiss`는 선택된 제안 조치, `see Alerts`는 승격된 anomaly가 Alerts 큐에 추천으로 올라와 있다는 뜻입니다.",
        ],
        HelpTopic::DashboardFooterProposalChip => &[
            "★p: 수락/거절 가능한 pending prompt-send 제안이 있는 pane 수입니다.",
            "포커스가 해당 pane/제안에 있을 때 p는 수락, d는 거절 경로로 들어갑니다.",
            "a Pending Actions overlay를 열면 모든 ★p 항목을 한 곳에서 보고 선택/일괄 처리할 수 있습니다.",
        ],
        HelpTopic::DashboardFooterCopyChip => &[
            "★y: 실행 가능한 suggested_command가 있어 y로 복사 가능한 alert 수입니다.",
            "Alerts 포커스에서 해당 alert를 선택하고 y를 누르면 표시된 run 명령을 클립보드로 복사합니다.",
            "a Pending Actions overlay에서도 ★y 항목을 모아 보고 선택한 항목을 복사할 수 있습니다.",
        ],
        HelpTopic::DashboardFooterAuditChip => &[
            "★a: 최근 15분 audit 이벤트의 최고 심각도입니다.",
            "0은 최근 유효 심각도 없음, C/W/R은 Concern/Warning/Risk를 뜻합니다.",
            "Warning/Risk는 색으로 강조되고, Concern/0은 상태 줄을 과도하게 끌어당기지 않도록 dim 처리됩니다.",
        ],
    }
}

fn en_lines(topic: HelpTopic) -> &'static [&'static str] {
    match topic {
        HelpTopic::AlertBulkHide => &[
            "bulk hide: hide or undo actionable alerts by severity.",
            "Click a Risk/Warning/Concern/Good chip to toggle that severity group.",
            "Pending auto-hide items can be undone before they disappear.",
        ],
        HelpTopic::AlertHeader => &[
            "header: timestamp, NEW badge, severity, and alert title in one row.",
            "The title names the alert type, including Recommendation, Checkpoint, or FLOW.",
            "FLOW groups related alerts that share one response path.",
            "The ★y chip means the alert has a copyable command suggestion.",
        ],
        HelpTopic::AlertDismiss => &[
            "dismiss: controls whether this alert is hidden or restored.",
            "[ ] means click/Enter/Space hides it; [x] means undo is available.",
            "The auto-hide countdown shows a scheduled hide.",
        ],
        HelpTopic::AlertSummary => &[
            "summary: the headline reason this alert exists.",
            "Examples include waiting for input, cross-pane file conflict, or cost/token pressure.",
        ],
        HelpTopic::AlertDetail => &[
            "details: extra next/run/anchor/others rows or FLOW rail rows below the summary.",
            "A FLOW rail shows cause signal -> follow-on evidence -> prerequisite action -> command.",
            "included rows list the original alerts used as evidence.",
        ],
        HelpTopic::AlertCopy => &[
            "copy: shown only when the selected alert has an executable suggested_command.",
            "With alert focus, press y to copy the displayed shell/slash command to the clipboard.",
        ],
        HelpTopic::PaneHeader => &[
            "pane header: tmux session/window, provider, role, CLI version, and pane ID.",
            "Qmonster panes intentionally omit CLI version.",
            "The ★p chip marks an accept/reject prompt-send proposal.",
        ],
        HelpTopic::PaneState => &[
            "state: idle/wait states such as IDLE DONE, WAIT INPUT, WAIT APPROVAL, USAGE LIMIT, IDLE STALE.",
            "The elapsed timer shows how long the pane has been in that state.",
            "STATE CHANGED/ACTIVE highlights recent transitions.",
        ],
        HelpTopic::PanePath => &[
            "path: the pane's current working directory.",
            "Long paths are shortened to fit; unknown paths render as unknown path.",
        ],
        HelpTopic::PaneCommand => &[
            "cmd: the current or most recently observed command in this pane.",
            "Long commands wrap within the card width.",
        ],
        HelpTopic::PaneStatus => &[
            "status: how confidently Qmonster identified the pane provider and role.",
            "high/medium/low/conflict confidence comes from the identity resolver.",
            "conflict means title and command evidence disagree, so provider-specific metrics were suppressed.",
        ],
        HelpTopic::PaneSignals => &[
            "signals: unusual terminal output or behavior patterns.",
            "Examples: waiting for input, approval needed, log storm, repeated output, error hint.",
        ],
        HelpTopic::PaneMetrics => &[
            "metrics: cost, context/quota pressure, reset ETA, model, branch/worktree, and related facts.",
            "COST changes color from Good through Risk as spend rises.",
            "[Official]/[Estimated] marks the source authority for each chip.",
        ],
        HelpTopic::PaneTokens => &[
            "tokens/cache io: input/output tokens and prompt-cache create/read stats.",
            "The selected pane also shows recent token movement and token io rows.",
        ],
        HelpTopic::PaneRuntime => &[
            "runtime facts: session, permission mode, sandbox, allowed dir, transcript, loaded tools/skills/plugins.",
            "session/loaded rows help trace the running agent and its injected capabilities.",
        ],
        HelpTopic::PaneRecommendation => &[
            "recommendations: issues, suggestions, and next steps found by Qmonster for this pane.",
            "Risk/Warning/Concern/Good/Safe labels are followed by reason and detail/next/run rows.",
        ],
        HelpTopic::DashboardDivider => &[
            "Alerts/Panes split divider: drag to resize the two panels.",
            "Keyboard: `[`/`]` shrink/grow Alerts, `/` cycle ratio, `=` reset.",
            "Typing a non-ASCII letter (Hangul/CJK/...) flips the divider into a `⚠ HANGUL/IME ACTIVE` banner and fires the terminal BEL once on activation (v1.51.0+, 3s TTL).",
        ],
        HelpTopic::DashboardFooter => &[
            "Footer: current focus, Alerts/Panes split ratio, pending-action/audit counters (★p / ★y / ★a), and the main keybinding cluster.",
            "★p counts acceptable prompt-send proposals; ★y counts alerts with a copyable suggested_command; ★a is recent audit severity.",
            "Keys are ` · `-separated; the focus marker flips to `overlay` while a modal owns the keyboard.",
        ],
        HelpTopic::DashboardVersionBadge => &[
            "Version badge: bottom-right tag/commit string from `git describe --tags --always --dirty`, embedded by build.rs.",
            "Click it to open the Git overlay — origin URL, branch/HEAD, working-tree changes, Recent Commits, Contributors.",
            "v1.51.0+ tracks `.git/refs/tags` so a fresh `git tag` triggers an automatic rebuild on next `cargo build`.",
        ],
        HelpTopic::DashboardNowStrip => &[
            "Now row: the topmost one-line summary of the most important current condition.",
            "Priority order is input/approval waits, Risk recommendations, quota/cost pressure, recent anomalies, then healthy status.",
            "`p send`/`d dismiss` point to proposal actions; `see Alerts` means the promoted anomaly is queued as a recommendation in Alerts.",
        ],
        HelpTopic::DashboardFooterProposalChip => &[
            "★p: count of panes with pending prompt-send proposals that can be accepted or dismissed.",
            "When focus is on the matching pane/proposal, p accepts and d dismisses through the proposal path.",
            "Open the a Pending Actions overlay to review, select, and bulk-dispatch all ★p items.",
        ],
        HelpTopic::DashboardFooterCopyChip => &[
            "★y: count of alerts with an executable suggested_command that can be copied with y.",
            "With Alerts focus on that alert, y copies the rendered run command to the clipboard.",
            "The a Pending Actions overlay also collects ★y items for review and selected copy dispatch.",
        ],
        HelpTopic::DashboardFooterAuditChip => &[
            "★a: highest severity from audit events in the recent 15-minute window.",
            "0 means no recent severity; C/W/R mean Concern, Warning, or Risk.",
            "Warning/Risk are severity-colored; Concern/0 stay dim so the status line remains calm.",
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_topic_has_korean_and_english_help() {
        let topics = [
            HelpTopic::AlertBulkHide,
            HelpTopic::AlertHeader,
            HelpTopic::AlertDismiss,
            HelpTopic::AlertSummary,
            HelpTopic::AlertDetail,
            HelpTopic::AlertCopy,
            HelpTopic::PaneHeader,
            HelpTopic::PaneState,
            HelpTopic::PanePath,
            HelpTopic::PaneCommand,
            HelpTopic::PaneStatus,
            HelpTopic::PaneSignals,
            HelpTopic::PaneMetrics,
            HelpTopic::PaneTokens,
            HelpTopic::PaneRuntime,
            HelpTopic::PaneRecommendation,
            HelpTopic::DashboardDivider,
            HelpTopic::DashboardFooter,
            HelpTopic::DashboardVersionBadge,
            HelpTopic::DashboardNowStrip,
            HelpTopic::DashboardFooterProposalChip,
            HelpTopic::DashboardFooterCopyChip,
            HelpTopic::DashboardFooterAuditChip,
        ];
        for topic in topics {
            assert!(!help_lines(topic, HelpLanguage::Ko).is_empty());
            assert!(!help_lines(topic, HelpLanguage::En).is_empty());
        }
    }

    #[test]
    fn alert_help_mentions_flow_rows_in_both_languages() {
        let ko_header = help_lines(HelpTopic::AlertHeader, HelpLanguage::Ko).join("\n");
        let ko_detail = help_lines(HelpTopic::AlertDetail, HelpLanguage::Ko).join("\n");
        let en_header = help_lines(HelpTopic::AlertHeader, HelpLanguage::En).join("\n");
        let en_detail = help_lines(HelpTopic::AlertDetail, HelpLanguage::En).join("\n");

        assert!(ko_header.contains("FLOW"));
        assert!(ko_header.contains("묶"));
        assert!(ko_detail.contains("실행 명령"));
        assert!(en_header.contains("FLOW"));
        assert!(en_header.contains("groups related alerts"));
        assert!(en_detail.contains("-> command"));
    }
}
