use embassy_executor::Spawner;
use embassy_time::{Instant, Timer};
use embedded_graphics::{
    geometry::Size,
    pixelcolor::BinaryColor,
    prelude::*,
    primitives::{Line, PrimitiveStyle, Rectangle, StyledDrawable},
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

#[embassy_executor::task(pool_size = 4)]
async fn draw_line(
    mut display: DisplayPartition<BinaryColor, SimulatorDisplay<BinaryColor>>,
) -> () {
    let max_x: i32 = (display.bounding_box().size.width - 1).try_into().unwrap();
    let max_y: i32 = (display.bounding_box().size.height - 1).try_into().unwrap();
    let start = Instant::now();
    loop {
        Line::new(Point::new(0, 0), Point::new(max_x, max_y))
            .draw_styled(
                &PrimitiveStyle::with_stroke(BinaryColor::On, 1),
                &mut display,
            )
            .await
            .unwrap();
        Timer::after_millis(500).await;
        Line::new(Point::new(0, max_y), Point::new(max_x, 0))
            .draw_styled(
                &PrimitiveStyle::with_stroke(BinaryColor::On, 1),
                &mut display,
            )
            .await
            .unwrap();
        Timer::after_millis(500).await;
        display.clear(BinaryColor::Off).await.unwrap();
        Timer::after_millis(500).await;

        if Instant::now().duration_since(start).as_secs() > 3 {
            break;
        }
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

    let (left_display, right_display) = shared_display.split_vertically(&mut display).unwrap();
    let right_area = right_display.partition;
    // TODO this should be the toolkit assign_app!
    spawner.must_spawn(draw_line(right_display));
    spawner.must_spawn(draw_line(left_display));

    let (lr_disp, rr_disp) = shared_display
        .split_existing_unchecked(&mut display, right_area)
        .unwrap();

    spawner.must_spawn(flush_simulator_display(display, window));

    // TODO is this not super dangerous? No guarantee that the previous task is done, what if
    Timer::after_secs(5).await;
    spawner.must_spawn(draw_line(lr_disp));
    spawner.must_spawn(draw_line(rr_disp));
}
