use embassy_executor::Spawner;
use embassy_time::Timer;
use embedded_graphics::{
    geometry::Size,
    mono_font::{ascii::FONT_10X20, MonoTextStyle},
    pixelcolor::BinaryColor,
    prelude::*,
    primitives::{Line, PrimitiveStyle, Rectangle, StyledDrawable},
    text::{Alignment, Baseline, Text, TextStyleBuilder},
};
use embedded_graphics_simulator::{
    BinaryColorTheme, OutputSettingsBuilder, SimulatorDisplay, SimulatorEvent, Window,
};
use shared_display::{
    sharable_display::DisplayPartition,
    toolkit::{App, SharedDisplay},
};

fn init_simulator_display() -> (SimulatorDisplay<BinaryColor>, Window) {
    let output_settings = OutputSettingsBuilder::new()
        .theme(BinaryColorTheme::OledWhite)
        .build();
    (
        SimulatorDisplay::new(Size::new(128, 64)),
        Window::new("Simulated Display", &output_settings),
    )
}

async fn flush_simulator_display(
    display: &mut SimulatorDisplay<BinaryColor>,
    window: &mut Window,
) -> bool {
    window.update(display);
    if window.events().any(|e| e == SimulatorEvent::Quit) {
        return false;
    }
    Timer::after_millis(50).await;
    true
}

struct LineApp {}

impl App for LineApp {
    type Display = DisplayPartition<BinaryColor, SimulatorDisplay<BinaryColor>>;

    async fn update_display(&self, display: &mut Self::Display) -> Rectangle {
        display.clear(BinaryColor::Off).await.unwrap();
        Line::new(Point::new(0, 0), Point::new(128, 128))
            .draw_styled(&PrimitiveStyle::with_stroke(BinaryColor::On, 1), display)
            .await
            .unwrap();
        Line::new(Point::new(0, 63), Point::new(63, 0))
            .draw_styled(&PrimitiveStyle::with_stroke(BinaryColor::On, 1), display)
            .await
            .unwrap();

        Rectangle::with_corners(Point::new(0, 0), Point::new(63, 63))
    }
}

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let (mut display, mut window) = init_simulator_display();

    let mut shared_display: SharedDisplay = SharedDisplay::new().await;

    let app_1 = LineApp {};
    let app_2 = LineApp {};

    let apps = vec![app_1, app_2];

    let right_rect = Rectangle::new(Point::new(64, 0), Size::new(64, 64));
    let right_display = shared_display
        .new_partition(&mut display, right_rect)
        .unwrap();

    let left_rect = Rectangle::new(Point::new(0, 0), Size::new(64, 64));
    let left_display = shared_display
        .new_partition(&mut display, left_rect)
        .unwrap();

    let mut displays = vec![left_display, right_display];

    assert_eq!(apps.len(), displays.len());

    loop {
        let mut total_updated_area: Option<Rectangle> = None;
        for (i, app) in apps.iter().enumerate() {
            let updated_area = app.update_display(&mut displays[i]).await;
            total_updated_area = Some(match total_updated_area {
                None => updated_area,
                Some(before) => before.envelope(&updated_area),
            })
        }
        if !flush_simulator_display(&mut display, &mut window).await {
            break;
        }
        Timer::after_millis(100).await;
    }
}
