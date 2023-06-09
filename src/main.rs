use anyhow::Context;
use serialport::SerialPort;
use std::io::{self, stdin, BufRead, BufReader};
use std::sync::mpsc;
use std::time::Duration;

enum Event {
    PrinterRx(std::io::Result<String>),
}

struct App {
    event_rx: mpsc::Receiver<Event>,
    printer: Box<dyn SerialPort>,
    _printer_rx_thread: std::thread::JoinHandle<()>,
    input_lines: io::Lines<io::StdinLock<'static>>,
    running: bool,
    printer_online: bool,
    pending_pause: bool,
    paused: bool,
}

impl App {
    fn new() -> anyhow::Result<Self> {
        let (event_tx, event_rx) = mpsc::channel::<Event>();
        let printer = serialport::new("/dev/ttyACM0", 115200)
            .timeout(Duration::from_secs(30))
            .open()
            .context("open printer")?;

        printer
            .clear(serialport::ClearBuffer::All)
            .context("open printer")?;

        let printer_rx = printer.try_clone().context("open printer")?;
        let printer_rx_event_tx = event_tx.clone();

        let _printer_rx_thread = std::thread::spawn(move || {
            let printer_rx = BufReader::new(printer_rx);
            for line_result in printer_rx.lines() {
                printer_rx_event_tx
                    .send(Event::PrinterRx(line_result))
                    .expect("printer recv event");
            }
        });

        Ok(Self {
            event_rx,
            printer,
            _printer_rx_thread,
            input_lines: stdin().lines(),
            running: false,
            printer_online: false,
            pending_pause: false,
            paused: false,
        })
    }

    fn send(&mut self, line: &str) -> anyhow::Result<()> {
        eprintln!("{}", line);
        write!(self.printer, "{}\r\n", line).context("printer send")?;
        Ok(())
    }

    fn send_line(&mut self) -> anyhow::Result<()> {
        if self.pending_pause {
            self.send("M601")?;
            self.pending_pause = false;
            self.paused = true;
            return Ok(());
        }

        loop {
            let Some(line_result) = self.input_lines.next() else { self.running = false; break; };
            let input_line = line_result.context("reading stdin")?;

            // Remove extra whitespace and trailing comments
            let trimmed_line = input_line
                .split_once(';')
                .map(|(before, _after)| before)
                .unwrap_or(&input_line)
                .trim();

            // Ignore lines with no content
            if trimmed_line.is_empty() {
                continue;
            }

            self.send(trimmed_line)?;
            break;
        }
        Ok(())
    }

    fn handle_event(&mut self, event: Event) -> anyhow::Result<()> {
        match event {
            Event::PrinterRx(line_result) => {
                let line = line_result.context("printer recv")?;
                self.handle_printer_rx(&line)?;
            }
        }
        Ok(())
    }

    fn handle_printer_rx(&mut self, line: &str) -> anyhow::Result<()> {
        let line = line.trim();
        println!("{}", line);
        if line.contains("action:cancel") {
            println!("USER CANCELLED");
            self.running = false;
        } else if line.contains("action:pause") {
            println!("PAUSED");
            self.pending_pause = true;
        } else if line.contains("action:resume") {
            println!("RESUMED");
            self.send("M602")?;
        } else if line.starts_with("start") {
            if !self.printer_online {
                self.printer_online = true;
                self.send_line()?;
            }
        } else if line.starts_with("ok") && !self.paused {
            self.send_line()?;
        }
        Ok(())
    }

    fn run(&mut self) -> anyhow::Result<()> {
        self.running = true;
        loop {
            if !self.running {
                break;
            }
            let event = self.event_rx.recv().context("event recv")?;
            self.handle_event(event)?;
        }
        Ok(())
    }
}

fn main() -> anyhow::Result<()> {
    let mut app = App::new()?;
    app.run()
}
