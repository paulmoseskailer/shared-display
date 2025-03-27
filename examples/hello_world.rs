use embassy_executor::Spawner;
use embassy_time::Timer;
use embedded_graphics::{
    geometry::Size, pixelcolor::BinaryColor, prelude::*, primitives::Rectangle,
};
use embedded_graphics_simulator::{
    BinaryColorTheme, OutputSettingsBuilder, SimulatorDisplay, Window,
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
async fn blink_display(
    mut display: DisplayPartition<BinaryColor, SimulatorDisplay<BinaryColor>>,
) -> () {
    loop {
        display.clear(BinaryColor::On).await.unwrap();
        Timer::after_millis(500).await;
        display.clear(BinaryColor::Off).await.unwrap();
        Timer::after_millis(500).await;
    }
}

#[embassy_executor::task]
async fn flush_simulator_display(mut display: SimulatorDisplay<BinaryColor>, mut window: Window) {
    window.update(&mut display);
}

const MAX_NUM_PARTITIONS: usize = 4;
type BufferElement = BinaryColor;
type DisplayType = SimulatorDisplay<BinaryColor>;
type MySharedDisplay = SharedDisplay<BufferElement, DisplayType, MAX_NUM_PARTITIONS>;

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let (mut display, mut window) = init_simulator_display();

    MySharedDisplay::init().await;

    let rect = Rectangle::new(Point::new(64, 0), Size::new(64, 64));
    if let Some(display_partition) = MySharedDisplay::new_partition(&mut display, rect).await {
        // TODO this should be the toolkit assign_app!
        spawner.must_spawn(blink_display(display_partition));
        // TODO this should be the toolkit flushing
        loop {
            window.update(&display);
            Timer::after_millis(50).await;
        }
    } else {
        println!("creating new partition failed!");
    }
}
