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
use shared_display::{sharable_display::DisplayPartition, toolkit::SharedDisplay};

fn init_simulator_display() -> (SimulatorDisplay<BinaryColor>, Window) {
    let output_settings = OutputSettingsBuilder::new()
        .theme(BinaryColorTheme::OledWhite)
        .build();
    (
        SimulatorDisplay::new(Size::new(128, 64)),
        Window::new("Simulated Display", &output_settings),
    )
}

#[embassy_executor::task]
async fn print_hello(
    mut display: DisplayPartition<BinaryColor, SimulatorDisplay<BinaryColor>>,
) -> () {
    let character_style = MonoTextStyle::new(&FONT_10X20, BinaryColor::On);
    let text_style = TextStyleBuilder::new()
        .baseline(Baseline::Middle)
        .alignment(Alignment::Center)
        .build();

    loop {
        Text::with_text_style(
            "hello \n world",
            Point::new(30, 20),
            character_style,
            text_style,
        )
        .draw(&mut display)
        .await
        .unwrap();
        Timer::after_millis(500).await;
        display.clear(BinaryColor::Off).await.unwrap();
        Timer::after_millis(500).await;
    }
}
#[embassy_executor::task]
async fn draw_line(
    mut display: DisplayPartition<BinaryColor, SimulatorDisplay<BinaryColor>>,
) -> () {
    loop {
        Line::new(Point::new(0, 0), Point::new(128, 128))
            .draw_styled(
                &PrimitiveStyle::with_stroke(BinaryColor::On, 1),
                &mut display,
            )
            .await
            .unwrap();
        Timer::after_millis(500).await;
        Line::new(Point::new(0, 63), Point::new(63, 0))
            .draw_styled(
                &PrimitiveStyle::with_stroke(BinaryColor::On, 1),
                &mut display,
            )
            .await
            .unwrap();
        Timer::after_millis(500).await;
        display.clear(BinaryColor::Off).await.unwrap();
        Timer::after_millis(500).await;
    }
}

#[embassy_executor::task]
async fn flush_simulator_display(mut display: SimulatorDisplay<BinaryColor>, mut window: Window) {
    loop {
        window.update(&mut display);
        if window.events().any(|e| e == SimulatorEvent::Quit) {
            break;
        }
        Timer::after_millis(50).await;
    }
}

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let (mut display, window) = init_simulator_display();

    let mut shared_display: SharedDisplay = SharedDisplay::new().await;

    let right_rect = Rectangle::new(Point::new(64, 0), Size::new(64, 64));
    if let Some(right_display) = shared_display.new_partition(&mut display, right_rect).await {
        // TODO this should be the toolkit assign_app!
        spawner.must_spawn(draw_line(right_display));
    } else {
        println!("creating new partition failed!");
    }

    let left_rect = Rectangle::new(Point::new(0, 0), Size::new(64, 64));
    if let Some(left_display) = shared_display.new_partition(&mut display, left_rect).await {
        // TODO this should be the toolkit assign_app!
        spawner.must_spawn(print_hello(left_display));
    } else {
        println!("creating new partition failed!");
    }

    spawner.must_spawn(flush_simulator_display(display, window));
}
