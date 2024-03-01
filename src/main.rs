use std::{
    cell::RefCell,
    ops::Deref,
    rc::Rc,
    sync::mpsc::{self, Receiver, Sender, TryRecvError},
    time::{Duration, Instant},
};

use pulse::{
    context::{Context, FlagSet as ContextFlagSet},
    def::Retval,
    mainloop::standard::{IterateResult, Mainloop},
    proplist::{properties, Proplist},
    sample::{Format, Spec},
    stream::{FlagSet as StreamFlagSet, Stream},
};

fn main() {
    let spec = Spec {
        format: Format::S16le,
        channels: 2,
        rate: 44100,
    };

    assert!(spec.is_valid());

    let mut proplist = Proplist::new().unwrap();
    proplist
        .set_str(properties::APPLICATION_NAME, "PulseVisualizer")
        .unwrap();

    let mainloop = Rc::new(RefCell::new(Mainloop::new().unwrap()));

    let context = Rc::new(RefCell::new(
        Context::new_with_proplist(
            mainloop.borrow().deref(),
            "PulseVisualizerContext",
            &proplist,
        )
        .unwrap(),
    ));

    context
        .borrow_mut()
        .connect(None, ContextFlagSet::NOFLAGS, None)
        .unwrap();

    loop {
        match mainloop.borrow_mut().iterate(false) {
            IterateResult::Err(_) | IterateResult::Quit(_) => {
                eprintln!("Iterate state was not success, qutting...");
                return;
            }
            IterateResult::Success(_) => {}
        }

        match context.borrow().get_state() {
            pulse::context::State::Ready => {
                break;
            }
            pulse::context::State::Failed | pulse::context::State::Terminated => {
                eprintln!("Context state failed/terminated, quitting...");
                return;
            }
            _ => {}
        }
    }

    println!("Creating stream.");

    let stream = Rc::new(RefCell::new(
        Stream::new(&mut context.borrow_mut(), "PulseVisualizer", &spec, None).unwrap(),
    ));

    println!("Getting sink.");
    let sink = get_sink(mainloop.clone(), context.clone());

    println!("Using sink {}.", sink);

    stream.borrow_mut().set_monitor_stream(sink).unwrap();

    stream
        .borrow_mut()
        .connect_record(None, None, StreamFlagSet::START_CORKED)
        .unwrap();

    // Wait for stream to be ready
    loop {
        match mainloop.borrow_mut().iterate(false) {
            IterateResult::Quit(_) | IterateResult::Err(_) => {
                eprintln!("Iterate state was not success, quitting...");
                return;
            }
            IterateResult::Success(_) => {}
        }
        match stream.borrow().get_state() {
            pulse::stream::State::Ready => {
                break;
            }
            pulse::stream::State::Failed | pulse::stream::State::Terminated => {
                eprintln!("Stream state failed/terminated, quitting...");
                return;
            }
            _ => {}
        }
    }

    let run_duration = Duration::from_secs(10);
    let start = Instant::now();

    println!("Running for {:?}.", run_duration);
    loop {
        match mainloop.borrow_mut().iterate(false) {
            IterateResult::Quit(_) | IterateResult::Err(_) => {
                eprintln!("Iterate state was not success, quitting...");
                return;
            }
            IterateResult::Success(_) => {}
        }

        if stream.borrow().is_corked().unwrap() {
            println!("Uncorking stream.");
            stream.borrow_mut().uncork(None);
        }

        if start.elapsed() > run_duration {
            break;
        }

        match stream.borrow_mut().peek() {
            Ok(peek_result) => match peek_result {
                pulse::stream::PeekResult::Empty => {}
                pulse::stream::PeekResult::Hole(_) => {
                    println!("Hole, discarding.");
                    stream.borrow_mut().discard().unwrap();
                }
                pulse::stream::PeekResult::Data(data) => {
                    if !data.iter().all(|&x| x == 0) {
                        println!("Received {} bytes of data: {:?}", data.len(), data);
                    }
                }
            },
            Err(err) => eprintln!("Error reading from stream {}.", err),
        }
    }

    println!("Shutdown.");

    mainloop.borrow_mut().quit(Retval(0));
    stream.borrow_mut().disconnect().unwrap();
}

fn get_sink(mainloop: Rc<RefCell<Mainloop>>, context: Rc<RefCell<Context>>) -> u32 {
    let (tx, rx): (Sender<u32>, Receiver<u32>) = mpsc::channel();
    context
        .borrow_mut()
        .introspect()
        .get_sink_info_list(move |result| match result {
            pulse::callbacks::ListResult::Item(item) => {
                if item.name.clone().unwrap().contains("FiiO") {
                    println!("Found sink {}.", item.name.clone().unwrap());
                    tx.send(item.index).unwrap();
                }
            }
            pulse::callbacks::ListResult::End => {}
            pulse::callbacks::ListResult::Error => eprintln!("Error getting sink info list."),
        });

    loop {
        match mainloop.borrow_mut().iterate(false) {
            IterateResult::Err(_) | IterateResult::Quit(_) => {
                panic!("Iterate state was not success, qutting...");
            }
            IterateResult::Success(_) => {}
        }

        match context.borrow().get_state() {
            pulse::context::State::Failed | pulse::context::State::Terminated => {
                panic!("Context state failed/terminated, quitting...");
            }
            _ => {}
        }

        match rx.try_recv() {
            Ok(sink) => break sink,
            Err(err) => match err {
                TryRecvError::Empty => {}
                TryRecvError::Disconnected => panic!("Empty channel."),
            },
        }
    }
}
