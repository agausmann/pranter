use anyhow::Context;
use serialport::SerialPort;
use std::io::{self, stdin, BufRead, BufReader};
use std::sync::mpsc;

enum Event {
    PrinterRx(std::io::Result<String>),
}

struct App {
    event_rx: mpsc::Receiver<Event>,
    printer: Box<dyn SerialPort>,
    _printer_rx_thread: std::thread::JoinHandle<()>,
    input_lines: io::Lines<io::StdinLock<'static>>,
    running: bool,
}

impl App {
    fn new() -> anyhow::Result<Self> {
        let (event_tx, event_rx) = mpsc::channel::<Event>();
        let printer = serialport::new("/dev/ttyACM0", 115200)
            .open()
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
        })
    }

    fn send_line(&mut self) -> anyhow::Result<()> {
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

            eprintln!("{}", trimmed_line);
            write!(self.printer, "{}\r\n", trimmed_line).context("printer send")?;
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
        println!("{}", line);
        if line.trim().starts_with("ok") {
            self.send_line()?;
        }
        Ok(())
    }

    fn run(&mut self) -> anyhow::Result<()> {
        self.running = true;
        self.send_line()?;
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
