//! A virtual pointer for tests in the nest (wtype only types):
//!   pointer <width> <height> move:<x>,<y> down up right sleep:<ms> …
//! Positions are in pixels of a `width` × `height` area mapped onto the output layout.

use wayland_client::globals::{registry_queue_init, GlobalListContents};
use wayland_client::protocol::wl_pointer::{Axis, ButtonState};
use wayland_client::protocol::wl_registry::WlRegistry;
use wayland_client::{delegate_noop, Connection, Dispatch, QueueHandle};
use wayland_protocols_wlr::virtual_pointer::v1::client::zwlr_virtual_pointer_manager_v1::ZwlrVirtualPointerManagerV1;
use wayland_protocols_wlr::virtual_pointer::v1::client::zwlr_virtual_pointer_v1::ZwlrVirtualPointerV1;

struct S;

impl Dispatch<WlRegistry, GlobalListContents> for S {
    fn event(_: &mut Self, _: &WlRegistry, _: wayland_client::protocol::wl_registry::Event, _: &GlobalListContents, _: &Connection, _: &QueueHandle<Self>) {}
}
delegate_noop!(S: ignore ZwlrVirtualPointerManagerV1);
delegate_noop!(S: ignore ZwlrVirtualPointerV1);

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    anyhow::ensure!(args.len() >= 3, "usage: pointer <width> <height> move:<x>,<y> down up right sleep:<ms> …");
    let (w, h): (u32, u32) = (args[0].parse()?, args[1].parse()?);
    let conn = Connection::connect_to_env()?;
    let (globals, mut queue) = registry_queue_init::<S>(&conn)?;
    let qh = queue.handle();
    let manager: ZwlrVirtualPointerManagerV1 = globals.bind(&qh, 1..=2, ())?;
    let pointer = manager.create_virtual_pointer(None, &qh, ());
    let _ = Axis::VerticalScroll;
    let mut t = 0u32;
    for step in &args[2..] {
        t += 20;
        match step.split_once(':') {
            Some(("move", at)) => {
                let (x, y) = at.split_once(',').ok_or_else(|| anyhow::anyhow!("move:<x>,<y>"))?;
                pointer.motion_absolute(t, x.parse()?, y.parse()?, w, h);
            }
            Some(("sleep", ms)) => {
                queue.roundtrip(&mut S)?;
                std::thread::sleep(std::time::Duration::from_millis(ms.parse()?));
                continue;
            }
            None if step == "down" => pointer.button(t, 0x110, ButtonState::Pressed),
            None if step == "up" => pointer.button(t, 0x110, ButtonState::Released),
            None if step == "right" => {
                pointer.button(t, 0x111, ButtonState::Pressed);
                pointer.frame();
                pointer.button(t + 5, 0x111, ButtonState::Released);
            }
            _ => anyhow::bail!("unknown step: {step}"),
        }
        pointer.frame();
        queue.roundtrip(&mut S)?;
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    queue.roundtrip(&mut S)?;
    Ok(())
}
