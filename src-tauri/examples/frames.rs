//! Grabs one tone-mapped frame per job through libmpv's GPU renderer.
//! Usage: frames <out_dir> <jobs.tsv>   where each line is name<TAB>seconds<TAB>path
//! Prototype of the thumbnail generator the app needs in Phase 1.

use libmpv2::{events::Event, Mpv};
use std::{env, fs, path::Path};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();
    let (out_dir, jobs) = (&args[1], fs::read_to_string(&args[2])?);
    fs::create_dir_all(out_dir)?;

    let mpv = Mpv::with_initializer(|init| {
        init.set_option("vo", "gpu-next")?;
        init.set_option("ao", "null")?;
        init.set_option("pause", "yes")?;
        init.set_option("force-window", "yes")?;
        init.set_option("window-minimized", "yes")?;
        init.set_option("geometry", "320x180")?;
        init.set_option("hwdec", "auto-safe")?;
        init.set_option("sid", "no")?;
        init.set_option("hr-seek", "yes")?;
        init.set_option("screenshot-format", "jpg")?;
        init.set_option("screenshot-jpeg-quality", "82")?;
        Ok(())
    })?;

    for line in jobs.lines().filter(|l| !l.trim().is_empty()) {
        let mut parts = line.splitn(3, '\t');
        let (name, seconds, path) = (parts.next().unwrap(), parts.next().unwrap(), parts.next().unwrap());
        let out = Path::new(out_dir).join(format!("{name}.jpg"));
        mpv.set_property("start", seconds)?;
        mpv.command("loadfile", &[path])?;
        let mut ok = false;
        // Wait until the seek target frame is on screen, then capture it.
        for _ in 0..400 {
            match mpv.wait_event(0.1) {
                Some(Ok(Event::PlaybackRestart)) => {
                    std::thread::sleep(std::time::Duration::from_millis(150));
                    mpv.command("screenshot-to-file", &[out.to_str().unwrap(), "video"])?;
                    ok = true;
                    break;
                }
                _ => {}
            }
        }
        println!("{} {name}", if ok { "ok  " } else { "FAIL" });
    }
    Ok(())
}
