use ratatui::style::Color;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Theme {
    pub prose: Color,
    pub strong: Color,
    pub heading: Color,
    pub subheading: Color,
    pub minor: Color,
    pub code: Color,
    pub link: Color,
    pub quote: Color,
    pub faint: Color,
    pub border: Color,
}

impl Default for Theme {
    fn default() -> Theme {
        Theme {
            prose: Color::Rgb(0xE6, 0xEB, 0xF3),
            strong: Color::Rgb(0xAD, 0xDB, 0x67),
            heading: Color::Rgb(0xD7, 0x77, 0x57),
            subheading: Color::Rgb(0xE4, 0xBA, 0x6A),
            minor: Color::Rgb(0xFF, 0xFF, 0xFF),
            code: Color::Rgb(0xAF, 0xD7, 0xFF),
            link: Color::Rgb(0xAF, 0xD7, 0xFF),
            quote: Color::Rgb(0x99, 0x99, 0x99),
            faint: Color::Rgb(0x6B, 0x6B, 0x6B),
            border: Color::Rgb(0x44, 0x44, 0x44),
        }
    }
}
