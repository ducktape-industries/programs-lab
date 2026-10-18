//! Writes the frames the view draws as fixtures for the app's
//! `--render-tree` (app `dev/screens`): `frames <dir>` leaves
//! `<dir>/manifest.json` and one `<name>.json` per screen. The app's real
//! renderer then draws them with no node behind it: the view runs here on
//! the tests' fake host over the real `huddle` program, driven to each
//! screen the way a person would get to it. The screens are the door's:
//! Start a call and Copy link in the room, the knocks at the door, and the
//! door itself at each stage, on the desktop and on a phone.
use std::cell::Cell;
use std::rc::Rc;

use ::program::{Env, MockHost, Program};
use huddle::{Huddle as Calls, Op};
use view::methods::{
    AudioDevice, AudioDevices, AudioKind, CameraDevice, CameraDevices, Change, Changes,
    ClipboardWrite, ClockTicks, Facing, HostFloat, HostId, HostRoute, HostSession,
    MicrophoneDevices, Placement, Query as Ask, Session, Submit,
};
use view::testing::{StreamSender, TestAppContext};

use huddle_view::Huddle;

const ME: u64 = 1;
const ADA: u64 = 2;
const BO: u64 = 3;
const CY: u64 = 4;
const MINTED: &str = "call-18f2c0a1-1";
const ADAS: &str = "call-ada";

const DESK: (f32, f32) = (768., 500.);
const PHONE: (f32, f32) = (390., 844.);

struct Line {
    cx: TestAppContext,
    chain: MockHost,
    now: Rc<Cell<u64>>,
    changes: StreamSender<Changes<Calls>>,
    route: StreamSender<HostRoute>,
    ticks: StreamSender<ClockTicks>,
}

fn env(now: u64) -> Env {
    Env {
        time: now * 1000,
        ..MockHost::env(huddle::PROGRAM)
    }
}

fn run(chain: &MockHost, now: u64, who: u64, op: Op) -> Result<(), view::host::Error> {
    let ctx = chain.exec(env(now).signed(who.to_be_bytes(), Some(who)));
    chain
        .attempt(|| Calls::execute(&ctx, op))
        .map_err(|refusal| view::host::Error::new(refusal.code, refusal.message))
}

/// The view open as account 1 over a chain with the huddle program and
/// people named Ada, Bo and Cy, on a host with a camera and a microphone.
fn line(touch: bool, dark: bool) -> Line {
    let mut cx = TestAppContext::new();
    cx.host().handle::<CameraDevices>(|()| {
        Ok(vec![CameraDevice {
            id: "0".into(),
            name: "Front Camera".into(),
            default: true,
            facing: Facing::Front,
        }])
    });
    cx.host().handle::<MicrophoneDevices>(|()| {
        Ok(AudioDevices {
            inputs: vec![AudioDevice {
                id: "mic".into(),
                name: "Built-in Microphone".into(),
                default: true,
                kind: AudioKind::Other,
            }],
            outputs: vec![AudioDevice {
                id: "out".into(),
                name: "Speakers".into(),
                default: true,
                kind: AudioKind::Speaker,
            }],
        })
    });
    cx.host().handle::<ClipboardWrite>(|_| Ok(()));
    // the pane stays on the desk: a float is drawn the same, smaller
    cx.host().handle::<HostFloat>(|_| {
        Ok(Placement {
            floating: false,
            on_top: false,
        })
    });
    cx.host().handle::<HostId>(|_| Ok(MINTED.into()));
    let chain = MockHost::default();
    let now = Rc::new(Cell::new(1_000));
    let (asked, at) = (chain.clone(), now.clone());
    cx.host().handle::<Ask<Calls>>(move |query| {
        Calls::query(&asked.query(env(at.get())), query)
            .map_err(|refusal| view::host::Error::new(refusal.code, refusal.message))
    });
    let (ran, at) = (chain.clone(), now.clone());
    cx.host().handle::<Submit<Calls>>(move |op| {
        Ok(match run(&ran, at.get(), ME, op) {
            Ok(()) => view::testing::applied(Vec::new()),
            Err(refusal) => view::methods::Submitted::Refused {
                frame: [0; 32],
                until: 0,
                refusal,
            },
        })
    });
    cx.host().handle::<Ask<identity::Identity>>(|query| {
        let person = |number, name: &str| identity::Profile {
            number,
            name: name.into(),
            kind: identity::Kind::User,
        };
        Ok(match query {
            identity::Query::Profile { number: ADA } => {
                identity::Reply::Profile(Some(person(ADA, "Ada Lovelace")))
            }
            identity::Query::Profile { number: BO } => {
                identity::Reply::Profile(Some(person(BO, "Bo Diddley")))
            }
            identity::Query::Profile { number: CY } => {
                identity::Reply::Profile(Some(person(CY, "Cy Twombly")))
            }
            identity::Query::Profile { .. } => identity::Reply::Profile(None),
            identity::Query::Profiles { .. } => identity::Reply::Profiles {
                profiles: vec![person(ADA, "Ada Lovelace"), person(BO, "Bo Diddley")],
                next: None,
            },
            query => panic!("unexpected identity query: {query:?}"),
        })
    });
    let ticks = cx.host().stream::<ClockTicks>();
    let sessions = cx.host().stream::<HostSession>();
    let changes = cx.host().stream::<Changes<Calls>>();
    let route = cx.host().stream::<HostRoute>();
    let (width, height) = if touch { PHONE } else { DESK };
    cx.simulate_resize(width, height);
    cx.open::<Huddle>();
    if touch {
        cx.simulate_touch(true);
    }
    cx.simulate_theme(dark);
    sessions.send(Session {
        connected: true,
        chain_id: "lab-0a1b2c3d".into(),
        signer: "0102".into(),
        account: Some(ME),
        endpoint: String::new(),
    });
    cx.run_until_parked();
    Line {
        cx,
        chain,
        now,
        changes,
        route,
        ticks,
    }
}

impl Line {
    fn block(&mut self, who: u64, op: Op) {
        if let Err(refusal) = run(&self.chain, self.now.get(), who, op) {
            panic!("refused: {refusal:?}");
        }
        let keys = vec![huddle::state::CALLS.key(&String::new())];
        self.changes.send(Some(Change { height: 2, keys }));
        self.cx.run_until_parked();
    }

    fn click(&mut self, id: &str) {
        self.cx.simulate_click(id);
        self.cx.run_until_parked();
    }

    /// Into the room with a call started alone, the camera off (the
    /// renderer has no camera: its black box would be the picture).
    fn started_alone(&mut self) {
        self.click("huddle-camera");
        self.click("huddle-join");
        self.click("huddle-start");
    }

    /// At Ada's door, by her call's link.
    fn at_adas_door(&mut self) {
        self.block(
            ADA,
            Op::Start {
                call: ADAS.into(),
                invited: vec![],
            },
        );
        self.route.send(format!("call/{ADAS}"));
        self.cx.run_until_parked();
    }

    fn knocked(&mut self) {
        self.at_adas_door();
        self.click("huddle-ask");
    }

    fn seconds(&mut self, n: u64) {
        for _ in 0..n {
            self.ticks.send(());
            self.cx.run_until_parked();
        }
    }
}

struct Screen {
    name: &'static str,
    touch: bool,
    dark: bool,
    drive: fn(&mut Line),
}

fn screens() -> Vec<Screen> {
    let mut screens = Vec::new();
    for (design, touch) in [("desk", false), ("phone", true)] {
        let screen = |stage: &'static str, dark, drive| Screen {
            name: Box::leak(format!("knock-{design}-{stage}").into_boxed_str()),
            touch,
            dark,
            drive,
        };
        screens.extend([
            screen("room-start", false, |line| {
                line.click("huddle-camera");
                line.click("huddle-join");
            }),
            screen("room-link", false, |line| line.started_alone()),
            screen("room-copied", true, |line| {
                line.started_alone();
                line.click("huddle-copy-link");
            }),
            screen("room-knocks", false, |line| {
                line.started_alone();
                line.block(
                    BO,
                    Op::Knock {
                        call: MINTED.into(),
                    },
                );
                line.block(
                    CY,
                    Op::Knock {
                        call: MINTED.into(),
                    },
                );
            }),
            screen("door-asking", false, |line| line.at_adas_door()),
            screen("door-waiting", false, |line| line.knocked()),
            screen("door-turned-away", true, |line| {
                line.knocked();
                line.block(
                    ADA,
                    Op::TurnAway {
                        call: ADAS.into(),
                        account: ME,
                    },
                );
            }),
            screen("door-no-answer", false, |line| {
                line.knocked();
                line.seconds(huddle::KNOCK_SECONDS);
            }),
        ]);
    }
    screens
}

fn main() {
    let dir = std::path::PathBuf::from(std::env::args().nth(1).expect("usage: frames <dir>"));
    std::fs::create_dir_all(&dir).expect("the fixtures directory");
    let mut manifest = Vec::new();
    for screen in screens() {
        let mut line = line(screen.touch, screen.dark);
        (screen.drive)(&mut line);
        line.cx.run_until_parked();
        let frame = serde_json::to_vec(&line.cx.whole_frame()).expect("a frame as JSON");
        std::fs::write(dir.join(format!("{}.json", screen.name)), frame).expect("the frame's file");
        let (width, height) = if screen.touch { PHONE } else { DESK };
        manifest.push(serde_json::json!({
            "name": screen.name,
            "width": width as u32,
            "height": height as u32,
            "theme": if screen.dark { "dark" } else { "light" },
        }));
        eprintln!("{}", screen.name);
    }
    let manifest = serde_json::to_vec_pretty(&manifest).expect("the manifest as JSON");
    std::fs::write(dir.join("manifest.json"), manifest).expect("the manifest's file");
}
