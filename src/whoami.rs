use colored::Colorize;
use indicatif::{ProgressBar, ProgressStyle};
use std::io::{self, Write};
use std::thread;
use std::time::Duration;

pub fn run_whoami() -> Result<(), Box<dyn std::error::Error>> {
    // ── Phase 1: Screen clear ──────────────────────────────────
    clear_screen();
    thread::sleep(Duration::from_millis(500));

    // ── Phase 2: Profile analysis ──────────────────────────────
    println!("Analyzing profile...\n");

    let profiles = ["rapper", "developer", "creator"];
    for p in &profiles {
        thread::sleep(Duration::from_millis(300));
        println!(" {} {}", "[ OK ]".green().bold(), p);
    }
    println!();

    // ── Phase 3: Progress bar ──────────────────────────────────
    println!("Compiling future...\n");
    let pb = ProgressBar::new(100);
    pb.set_style(
        ProgressStyle::with_template("{bar:40.cyan/blue} {percent}%")
            .unwrap()
            .progress_chars("█▓▒░ "),
    );
    for i in 0..=100 {
        thread::sleep(Duration::from_millis(8));
        pb.set_position(i);
    }
    pb.finish_and_clear();
    println!();

    // ── Phase 4: Press ENTER ────────────────────────────────────
    print!("Press ENTER to continue.");
    io::stdout().flush()?;
    let mut _input = String::new();
    io::stdin().read_line(&mut _input)?;

    // ── Phase 5: ASCII Art — XAZZ IT BASE ───────────────────────
    clear_screen();

    let xazz = [
        "██╗  ██╗██╗████████╗",
        "██║  ██║██║╚══██╔══╝",
        "███████║██║   ██║   ",
        "██╔══██║██║   ██║   ",
        "██║  ██║██║   ██║   ",
        "╚═╝  ╚═╝╚═╝   ╚═╝   ",
    ];

    let it = [
        "        ██╗████████╗",
        "        ██║╚══██╔══╝",
        "        ██║   ██║   ",
        "        ██║   ██║   ",
        "        ██║   ██║   ",
        "        ╚═╝   ╚═╝   ",
    ];

    let base = [
        "██████╗  █████╗ ███████╗███████╗",
        "██╔══██╗██╔══██╗██╔════╝██╔════╝",
        "██████╔╝███████║███████╗█████╗  ",
        "██╔══██╗██╔══██║╚════██║██╔══╝  ",
        "██████╔╝██║  ██║███████║███████╗",
        "╚═════╝ ╚═╝  ╚═╝╚══════╝╚══════╝",
    ];

    thread::sleep(Duration::from_millis(200));
    println!("Generating identity...\n");

    for line in &xazz {
        println!("{}", line.cyan());
    }

    println!();

    for line in &it {
        println!("{}", line.cyan());
    }

    println!();

    for line in &base {
        println!("{}", line.cyan());
    }

    println!("\n");
    thread::sleep(Duration::from_millis(300));
    println!("{}", "Identity compiled successfully.".green().bold());

    thread::sleep(Duration::from_millis(300));

    // ── Phase 6: Box art ────────────────────────────────────────
    println!("{}", "╔══════════════════════════════╗".cyan());
    println!("{}", "║                              ║".cyan());
    println!(
        "{}",
        format!("║{}║", "      THE COMBINATION         ".white().bold()).cyan()
    );
    println!(
        "{}",
        format!("║{}║", "             OF               ".white().bold()).cyan()
    );
    println!(
        "{}",
        format!("║{}║", "     ART AND TECHNOLOGY       ".white().bold()).cyan()
    );
    println!("{}", "║                              ║".cyan());
    println!("{}", "╚══════════════════════════════╝".cyan());

    thread::sleep(Duration::from_millis(500));

    // ── Phase 7: Beat drop ──────────────────────────────────────

    // ── Phase 8: User fills in video playback ─────────────────
    // [User Note]: Insert video playback logic here
    //   e.g., std::process::Command::new("start")
    //       .arg("path/to/school_festival_rap.mp4")
    //       .spawn()?;

    Ok(())
}

fn clear_screen() {
    #[cfg(target_os = "windows")]
    {
        let _ = std::process::Command::new("cmd")
            .args(["/c", "cls"])
            .status();
    }
    #[cfg(not(target_os = "windows"))]
    {
        print!("\x1b[2J\x1b[1;1H");
    }
}
