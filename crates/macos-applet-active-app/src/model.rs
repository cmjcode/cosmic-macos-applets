// SPDX-License-Identifier: GPL-3.0-only
//! Pure, I/O-free logic: which window counts as "active" and how its label is shown.

/// Protocol object id of a toplevel handle; stable for the handle's lifetime.
pub type ToplevelId = u32;

/// A thread-safe snapshot of one toplevel window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Toplevel {
    pub id: ToplevelId,
    pub app_id: String,
    pub title: String,
    pub activated: bool,
    pub minimized: bool,
    /// Names of the outputs (e.g. `eDP-1`) the window is visible on.
    pub outputs: Vec<String>,
}

/// Remembers the order in which windows were activated (most recent last).
#[derive(Debug, Default, Clone)]
pub struct FocusHistory {
    order: Vec<ToplevelId>,
}

impl FocusHistory {
    /// Update the history from a fresh snapshot of all toplevels.
    pub fn observe(&mut self, toplevels: &[Toplevel]) {
        self.order
            .retain(|id| toplevels.iter().any(|t| t.id == *id));
        if let Some(active) = toplevels.iter().find(|t| t.activated)
            && self.order.last() != Some(&active.id)
        {
            self.order.retain(|id| *id != active.id);
            self.order.push(active.id);
        }
    }

    /// Pick the window whose name the panel on `panel_output` should show.
    ///
    /// * Nothing focused (the desktop was clicked) shows nothing.
    /// * The focused window wins if it is on this output, or when output
    ///   following is disabled or the panel's output is unknown.
    /// * Otherwise, when following the output, fall back to the most recently
    ///   focused, non-minimized window on this output, the way macOS keeps the
    ///   last active app on a secondary display.
    #[must_use]
    pub fn select<'a>(
        &self,
        toplevels: &'a [Toplevel],
        panel_output: Option<&str>,
        follow_output: bool,
    ) -> Option<&'a Toplevel> {
        let output = panel_output.filter(|o| follow_output && !o.is_empty());
        let on_output = |t: &Toplevel| {
            output.is_none_or(|o| t.outputs.is_empty() || t.outputs.iter().any(|n| n == o))
        };

        let active = toplevels.iter().find(|t| t.activated)?;
        if on_output(active) {
            return Some(active);
        }

        self.order.iter().rev().find_map(|id| {
            toplevels
                .iter()
                .find(|t| t.id == *id && !t.minimized && on_output(t))
        })
    }
}

/// Shorten `label` to at most `max_chars` characters, ending with an ellipsis.
#[must_use]
pub fn ellipsize(label: &str, max_chars: usize) -> String {
    let max_chars = max_chars.max(2);
    if label.chars().count() <= max_chars {
        return label.to_owned();
    }
    let kept: String = label.chars().take(max_chars - 1).collect();
    format!("{}…", kept.trim_end())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn win(id: u32, app: &str, activated: bool, output: &str) -> Toplevel {
        Toplevel {
            id,
            app_id: app.into(),
            title: format!("{app} window"),
            activated,
            minimized: false,
            outputs: vec![output.into()],
        }
    }

    #[test]
    fn active_window_on_same_output_is_selected() {
        let wins = [
            win(1, "firefox", false, "DP-1"),
            win(2, "code", true, "DP-1"),
        ];
        let mut h = FocusHistory::default();
        h.observe(&wins);
        assert_eq!(h.select(&wins, Some("DP-1"), true).unwrap().id, 2);
    }

    #[test]
    fn falls_back_to_last_focused_on_this_output() {
        let mut h = FocusHistory::default();
        let step1 = [
            win(1, "firefox", true, "DP-1"),
            win(2, "code", false, "HDMI-1"),
        ];
        h.observe(&step1);
        let step2 = [
            win(1, "firefox", false, "DP-1"),
            win(2, "code", true, "HDMI-1"),
        ];
        h.observe(&step2);

        assert_eq!(
            h.select(&step2, Some("DP-1"), true).unwrap().app_id,
            "firefox"
        );
        assert_eq!(
            h.select(&step2, Some("HDMI-1"), true).unwrap().app_id,
            "code"
        );
        // Not following outputs: always the globally focused window.
        assert_eq!(
            h.select(&step2, Some("DP-1"), false).unwrap().app_id,
            "code"
        );
    }

    #[test]
    fn minimized_and_closed_windows_are_skipped() {
        let mut h = FocusHistory::default();
        h.observe(&[win(1, "a", true, "DP-1"), win(2, "b", false, "HDMI-1")]);
        let mut a = win(1, "a", false, "DP-1");
        a.minimized = true;
        let now = [a, win(2, "b", true, "HDMI-1")];
        h.observe(&now);
        assert!(h.select(&now, Some("DP-1"), true).is_none());

        h.observe(&[win(2, "b", true, "HDMI-1")]);
        assert_eq!(h.order, vec![2]);
    }

    #[test]
    fn nothing_focused_means_desktop() {
        let wins = [win(1, "a", false, "DP-1")];
        let mut h = FocusHistory::default();
        h.observe(&wins);
        assert!(h.select(&wins, Some("DP-1"), true).is_none());
    }

    #[test]
    fn unknown_panel_output_does_not_hide_focused_window() {
        let wins = [win(1, "a", true, "DP-1")];
        let h = FocusHistory::default();
        assert_eq!(h.select(&wins, Some(""), true).unwrap().id, 1);
        assert_eq!(h.select(&wins, None, true).unwrap().id, 1);
    }

    #[test]
    fn ellipsize_respects_char_boundaries() {
        assert_eq!(ellipsize("Files", 10), "Files");
        assert_eq!(ellipsize("Visual Studio Code", 8), "Visual…");
        assert_eq!(ellipsize("日本語テキストエディタ", 4), "日本語…");
        assert_eq!(ellipsize("abc", 0), "a…");
    }
}
