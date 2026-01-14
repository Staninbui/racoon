//! Theme definitions for the TUI.
//!
//! Provides color palettes for dark and light themes.

use ratatui::style::{Color, Modifier, Style};

/// Available themes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Theme {
    #[default]
    Dark,
    Light,
}

impl Theme {
    /// Parse theme from string.
    pub fn parse(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "light" => Theme::Light,
            _ => Theme::Dark,
        }
    }
}

/// Color palette for UI elements.
#[derive(Debug, Clone)]
pub struct ThemeColors {
    // Base colors
    pub fg: Color,
    pub bg: Color,
    pub fg_dim: Color,

    // UI elements
    pub border: Color,
    pub border_focused: Color,
    pub header_fg: Color,
    pub header_bg: Color,

    // Log severity colors
    pub severity_debug: Color,
    pub severity_info: Color,
    pub severity_notice: Color,
    pub severity_warning: Color,
    pub severity_error: Color,
    pub severity_critical: Color,
    pub severity_default: Color,

    // Selection
    pub selection_fg: Color,
    pub selection_bg: Color,
    pub search_match_bg: Color,

    // AI pane
    pub ai_fg: Color,
    pub ai_border: Color,
    pub ai_loading: Color,

    // Status
    pub status_live: Color,
    pub status_paused: Color,
    pub error: Color,
    pub success: Color,
}

impl Theme {
    /// Get the color palette for this theme.
    pub fn colors(&self) -> ThemeColors {
        match self {
            Theme::Dark => dark_theme(),
            Theme::Light => light_theme(),
        }
    }
}

/// Dark theme color palette.
fn dark_theme() -> ThemeColors {
    ThemeColors {
        // Base
        fg: Color::Rgb(220, 220, 220), // Bright white for better visibility
        bg: Color::Reset,
        fg_dim: Color::Rgb(140, 140, 140), // Brighter gray for better contrast on dark terminals

        // UI elements
        border: Color::DarkGray,
        border_focused: Color::Cyan,
        header_fg: Color::Cyan,
        header_bg: Color::Reset,

        // Log severity
        severity_debug: Color::DarkGray,
        severity_info: Color::Blue,
        severity_notice: Color::Cyan,
        severity_warning: Color::Yellow,
        severity_error: Color::Red,
        severity_critical: Color::Magenta,
        severity_default: Color::White,

        // Selection
        selection_fg: Color::Black,
        selection_bg: Color::White,
        search_match_bg: Color::DarkGray,

        // AI pane
        ai_fg: Color::Cyan,
        ai_border: Color::Yellow,
        ai_loading: Color::Yellow,

        // Status
        status_live: Color::Green,
        status_paused: Color::Yellow,
        error: Color::Red,
        success: Color::Green,
    }
}

/// Light theme color palette.
fn light_theme() -> ThemeColors {
    ThemeColors {
        // Base
        fg: Color::LightBlue,
        bg: Color::White,
        fg_dim: Color::Gray,

        // UI elements
        border: Color::Gray,
        border_focused: Color::Blue,
        header_fg: Color::Blue,
        header_bg: Color::White,

        // Log severity
        severity_debug: Color::Gray,
        severity_info: Color::Green,
        severity_notice: Color::Cyan,
        severity_warning: Color::Yellow,
        severity_error: Color::Rgb(180, 0, 0),      // Dark red
        severity_critical: Color::Rgb(128, 0, 128), // Purple
        severity_default: Color::Black,

        // Selection
        selection_fg: Color::White,
        selection_bg: Color::Blue,
        search_match_bg: Color::LightYellow,

        // AI pane
        ai_fg: Color::Rgb(0, 100, 100),     // Teal
        ai_border: Color::Rgb(180, 140, 0), // Gold
        ai_loading: Color::Rgb(180, 140, 0),

        // Status
        status_live: Color::Rgb(0, 140, 0), // Dark green
        status_paused: Color::Rgb(180, 140, 0),
        error: Color::Rgb(180, 0, 0),
        success: Color::Rgb(0, 140, 0),
    }
}

impl ThemeColors {
    /// Get severity color based on severity string.
    pub fn severity_color(&self, severity: &str) -> Color {
        match severity.to_uppercase().as_str() {
            "DEBUG" => self.severity_debug,
            "INFO" => self.severity_info,
            "NOTICE" => self.severity_notice,
            "WARNING" | "WARN" => self.severity_warning,
            "ERROR" => self.severity_error,
            "CRITICAL" | "ALERT" | "EMERGENCY" => self.severity_critical,
            _ => self.severity_default,
        }
    }

    /// Get style for severity.
    pub fn severity_style(&self, severity: &str) -> Style {
        Style::default().fg(self.severity_color(severity))
    }

    /// Style for borders.
    pub fn border_style(&self) -> Style {
        Style::default().fg(self.border)
    }

    /// Style for focused borders.
    pub fn border_focused_style(&self) -> Style {
        Style::default().fg(self.border_focused)
    }

    /// Style for header.
    pub fn header_style(&self) -> Style {
        Style::default()
            .fg(self.header_fg)
            .add_modifier(Modifier::BOLD)
    }

    /// Style for selected item.
    pub fn selection_style(&self) -> Style {
        Style::default()
            .fg(self.selection_fg)
            .bg(self.selection_bg)
            .add_modifier(Modifier::BOLD)
    }

    /// Style for search matches.
    pub fn search_match_style(&self) -> Style {
        Style::default().bg(self.search_match_bg)
    }

    /// Style for AI response.
    pub fn ai_style(&self) -> Style {
        Style::default().fg(self.ai_fg)
    }

    /// Style for dim/hint text.
    pub fn dim_style(&self) -> Style {
        Style::default().fg(self.fg_dim)
    }

    /// Style for error messages.
    pub fn error_style(&self) -> Style {
        Style::default().fg(self.error)
    }

    /// Style for success messages.
    pub fn success_style(&self) -> Style {
        Style::default().fg(self.success)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_theme_dark() {
        let theme = Theme::Dark;
        let colors = theme.colors();
        assert_eq!(colors.fg, Color::Rgb(220, 220, 220));
        assert_eq!(colors.bg, Color::Reset);
        assert_eq!(colors.fg_dim, Color::Rgb(140, 140, 140));
    }

    #[test]
    fn test_theme_white() {
        let theme = Theme::Light;
        let colors = theme.colors();
        assert_eq!(colors.fg, Color::Black);
        assert_eq!(colors.bg, Color::White);
        assert_eq!(colors.fg_dim, Color::Gray);
    }
}
