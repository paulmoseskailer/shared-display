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
use shared_display::{DisplayPartition, FlushResult, SharedDisplay, launch_app_in_app};

type DisplayType = SimulatorDisplay<BinaryColor>;

fn init_simulator_display() -> (DisplayType, Window) {
    let output_settings = OutputSettingsBuilder::new()
        .theme(BinaryColorTheme::OledWhite)
        .build();
    (
        SimulatorDisplay::new(Size::new(128, 64)),
        Window::new("Simulated Display", &output_settings),
    )
}

async fn recursive_split_app(
    recursion_level: u8,
    mut display: DisplayPartition<BinaryColor, DisplayType>,
    spawner: &'static Spawner,
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
        Timer::after_millis(200).await;
        Line::new(Point::new(0, max_y), Point::new(max_x, 0))
            .draw_styled(
                &PrimitiveStyle::with_stroke(BinaryColor::On, 1),
                &mut display,
            )
            .await
            .unwrap();
        Timer::after_millis(500).await;
        display.clear(BinaryColor::Off).await.unwrap();
        Timer::after_millis(200).await;

        if recursion_level > 0 && Instant::now().duration_since(start).as_secs() > 1 {
            break;
        }
    }
    // recursive case
    let (left_display, right_display) = display.split_vertically().unwrap();
    let new_recursion_level = recursion_level - 1;
    launch_app_in_app(
        spawner,
        move |d| recursive_split_app(new_recursion_level, d, spawner),
        left_display,
    )
    .await;
    launch_app_in_app(
        spawner,
        move |d| recursive_split_app(new_recursion_level, d, spawner),
        right_display,
    )
    .await;
}

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let (display, mut window) = init_simulator_display();

    let mut shared_display: SharedDisplay<DisplayType> = SharedDisplay::new(display, spawner);

    let half_size = Size::new(64, 64);
    let left_rect = Rectangle::new(Point::new(0, 0), half_size);
    let right_rect = Rectangle::new(Point::new(64, 0), half_size);
    shared_display
        .launch_new_recursive_app(
            move |disp, spawner| recursive_split_app(2, disp, spawner),
            left_rect,
        )
        .await
        .unwrap();
    shared_display
        .launch_new_recursive_app(
            move |disp, spawner| recursive_split_app(1, disp, spawner),
            right_rect,
        )
        .await
        .unwrap();

    shared_display
        .run_flush_loop_with(async |d, _area| {
            window.update(d);
            if window.events().any(|e| e == SimulatorEvent::Quit) {
                return FlushResult::Abort;
            }
            FlushResult::Continue
        })
        .await;
}
