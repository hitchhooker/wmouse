use wayland_client::{
    protocol::{wl_pointer, wl_seat, wl_registry},
    Connection, Dispatch, QueueHandle,
};
use wayland_protocols_wlr::virtual_pointer::v1::client::{
    zwlr_virtual_pointer_manager_v1,
    zwlr_virtual_pointer_v1,
};
use input::{
    event::{
        keyboard::{KeyboardEventTrait},
        KeyboardEvent, Event,
    },
    Libinput, LibinputInterface
};
use std::{
    collections::HashSet,
    fs::OpenOptions,
    os::unix::prelude::*,
    path::Path,
    thread,
    time::Duration,
};

// Key mappings
const META_KEY: u16 = 125;        // Usually Windows/Super key
const MOVE_LEFT: u16 = 105;       // Left arrow
const MOVE_RIGHT: u16 = 106;      // Right arrow
const MOVE_UP: u16 = 103;         // Up arrow
const MOVE_DOWN: u16 = 108;       // Down arrow
const MOUSE_LEFT: u16 = 97;       // Right Control
const MOUSE_RIGHT: u16 = 96;      // Right Shift

// Mouse settings
const MOUSE_SPEED: f64 = 10.0;
const SLEEP_MS: u64 = 8;

struct InputHandler;

impl LibinputInterface for InputHandler {
    fn open_restricted(&mut self, path: &Path, flags: i32) -> Result<OwnedFd, i32> {
        OpenOptions::new()
            .read((flags & libc::O_RDONLY) != 0)
            .write((flags & libc::O_RDWR) != 0)
            .open(path)
            .map(|file| file.into())
            .map_err(|err| err.raw_os_error().unwrap_or(-1))
    }

    fn close_restricted(&mut self, fd: OwnedFd) {
        drop(fd);
    }
}

#[derive(Default)]
struct MouseState {
    dx: f64,
    dy: f64,
    left_click: bool,
    right_click: bool,
    x: f64,
    y: f64,
}

struct State {
    pointer_manager: Option<zwlr_virtual_pointer_manager_v1::ZwlrVirtualPointerManagerV1>,
    virtual_pointer: Option<zwlr_virtual_pointer_v1::ZwlrVirtualPointerV1>,
    active_keys: HashSet<u16>,
    prev_left_click: bool,
    prev_right_click: bool,
}

impl State {
    fn new() -> Self {
        Self {
            pointer_manager: None,
            virtual_pointer: None,
            active_keys: HashSet::new(),
            prev_left_click: false,
            prev_right_click: false,
        }
    }

    fn update_and_handle_mouse_state(&mut self) -> MouseState {
        let mut state = MouseState::default();

        if !self.active_keys.contains(&META_KEY) {
            return state;
        }

        // Update movement
        if self.active_keys.contains(&MOVE_LEFT) { state.dx -= MOUSE_SPEED; }
        if self.active_keys.contains(&MOVE_RIGHT) { state.dx += MOUSE_SPEED; }
        if self.active_keys.contains(&MOVE_UP) { state.dy -= MOUSE_SPEED; }
        if self.active_keys.contains(&MOVE_DOWN) { state.dy += MOUSE_SPEED; }

        // Update absolute position
        state.x += state.dx;
        state.y += state.dy;

        // Clamp coordinates to screen bounds (assuming 1920x1080)
        state.x = state.x.max(0.0).min(1920.0);
        state.y = state.y.max(0.0).min(1080.0);

        // Update button states
        state.left_click = self.active_keys.contains(&MOUSE_LEFT);
        state.right_click = self.active_keys.contains(&MOUSE_RIGHT);

        // Handle the movement
        if let Some(virtual_pointer) = &self.virtual_pointer {
            if state.dx != 0.0 || state.dy != 0.0 {
                virtual_pointer.motion_absolute(
                    0,  // time
                    (state.x * 65535.0 / 1920.0) as u32,  // x normalized to 0-65535
                    (state.y * 65535.0 / 1080.0) as u32,  // y normalized to 0-65535
                    65535,  // width denominator
                    65535,  // height denominator
                );
                virtual_pointer.frame();
            }
        }

        // Handle button state changes
        if let Some(virtual_pointer) = &self.virtual_pointer {
            const BTN_LEFT: u32 = 0x110;   // Standard Linux button codes
            const BTN_RIGHT: u32 = 0x111;

            // Left click changed
            if state.left_click != self.prev_left_click {
                let state_val = if state.left_click { wl_pointer::ButtonState::Pressed } else { wl_pointer::ButtonState::Released };
                virtual_pointer.button(0, BTN_LEFT, state_val);
                virtual_pointer.frame();
                self.prev_left_click = state.left_click;
            }

            // Right click changed
            if state.right_click != self.prev_right_click {
                let state_val = if state.right_click { wl_pointer::ButtonState::Pressed } else { wl_pointer::ButtonState::Released };
                virtual_pointer.button(0, BTN_RIGHT, state_val);
                virtual_pointer.frame();
                self.prev_right_click = state.right_click;
            }
        }

        state
    }
}

impl Dispatch<wl_pointer::WlPointer, ()> for State {
    fn event(_: &mut Self, _: &wl_pointer::WlPointer, _: wl_pointer::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}

impl Dispatch<wl_seat::WlSeat, ()> for State {
    fn event(
        state: &mut Self,
        _: &wl_seat::WlSeat,
        event: wl_seat::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_seat::Event::Capabilities { capabilities } = event {
            if let Ok(caps) = capabilities.into_result() {
                if caps.contains(wl_seat::Capability::Pointer) && 
                   state.virtual_pointer.is_none() && 
                   state.pointer_manager.is_some() 
                {
                    // Create virtual pointer with the manager
                    let vptr = state.pointer_manager.as_ref().unwrap()
                        .create_virtual_pointer(None, qh, ());
                    state.virtual_pointer = Some(vptr);
                }
            }
        }
    }
}

impl Dispatch<zwlr_virtual_pointer_manager_v1::ZwlrVirtualPointerManagerV1, ()> for State {
    fn event(_: &mut Self, _: &zwlr_virtual_pointer_manager_v1::ZwlrVirtualPointerManagerV1, _: zwlr_virtual_pointer_manager_v1::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}

impl Dispatch<zwlr_virtual_pointer_v1::ZwlrVirtualPointerV1, ()> for State {
    fn event(_: &mut Self, _: &zwlr_virtual_pointer_v1::ZwlrVirtualPointerV1, _: zwlr_virtual_pointer_v1::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}

impl Dispatch<wl_registry::WlRegistry, ()> for State {
    fn event(
        state: &mut Self,
        registry: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_registry::Event::Global { name, interface, .. } = event {
            match interface.as_str() {
                "zwlr_virtual_pointer_manager_v1" => {
                    state.pointer_manager = Some(registry.bind(
                        name,
                        1,
                        qh,
                        (),
                    ));
                }
                _ => {},
            }
        }
    }
}

fn process_input_events(input: &mut Libinput, state: &mut State) {
    while let Some(event) = input.next() {
        if let Event::Keyboard(KeyboardEvent::Key(key_event)) = event {
            match key_event.key_state() {
                input::event::keyboard::KeyState::Pressed => { 
                    state.active_keys.insert(key_event.key() as u16); 
                }
                input::event::keyboard::KeyState::Released => { 
                    state.active_keys.remove(&(key_event.key() as u16)); 
                }
            }
        }
    }
}

#[derive(Debug)]
struct WaylandError;
impl std::error::Error for WaylandError {}
impl std::fmt::Display for WaylandError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "Wayland error occurred")
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Set up Wayland connection
    let conn = Connection::connect_to_env()?;
    let display = conn.display();
    let mut event_queue = conn.new_event_queue();
    let qh = event_queue.handle();

    let mut state = State::new();
    display.get_registry(&qh, ());
    event_queue.roundtrip(&mut state)?;

    let mut input = Libinput::new_with_udev(InputHandler);
    input.udev_assign_seat("seat0").map_err(|_| WaylandError)?;

    // Main loop
    loop {
        process_input_events(&mut input, &mut state);
        state.update_and_handle_mouse_state();
        event_queue.dispatch_pending(&mut state)?;
        thread::sleep(Duration::from_millis(SLEEP_MS));
    }
}
