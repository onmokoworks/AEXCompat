// A worker whose stderr runs past the broker's capture bound, so the
// production capture's retention can be checked end to end (issue #1290):
// worker stderr is a trace, and what a failing selector did last is at its
// end, so the bytes the broker keeps have to be the last ones.
//
// Numbered lines rather than a fill byte: the test's whole question is *which*
// end survived, which a uniform fill cannot answer. The first and last lines
// are distinctive so the test can name both ends.
//
// The second argument is the broker's retention bound, and it is what makes
// the other half of the check deterministic. The capture keeps exactly the
// last `bound` bytes, so this places one `C:\...` path to straddle that
// boundary: its drive letter falls in the dropped part and its tail in the
// kept part. `redact_windows_paths` only recognises a path at its drive
// letter, so unless the broker realigns the capture to a line boundary first,
// that private path arrives in the report unredacted. The straddling path's
// stream offset and the total length go to stdout so the test can assert the
// arrangement it depends on rather than assume it.
use std::io::{self, Write};

const STRADDLING: &str = "opened C:\\private\\worker\\trace.txt here\n";
// How much of the straddling line has to land on the kept side for the cut to
// fall between the drive letter and the end of the path.
const AFTER_DRIVE_LETTER: usize = "opened C:\\".len();

fn line(number: u64) -> String {
    if number % 8 == 0 {
        format!("stage:line {number} at C:\\private\\worker\\{number}.txt\n")
    } else {
        format!("stage:line {number}\n")
    }
}

fn main() -> io::Result<()> {
    let mut arguments = std::env::args().skip(1);
    let mut number = |name: &str| -> io::Result<usize> {
        arguments
            .next()
            .and_then(|value| value.parse::<usize>().ok())
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, name.to_owned()))
    };
    let lead_bytes = number("lead byte count required")?;
    let bound = number("retention bound required")?;
    let last = "stage:the_last_thing_it_did\n";
    let kept_from_straddling = STRADDLING.len() - AFTER_DRIVE_LETTER;
    if bound <= kept_from_straddling + last.len() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "the retention bound has to hold the straddling tail and the last line",
        ));
    }
    // Everything after the straddling line, sized so that the boundary lands
    // inside it.
    let mut trailing = String::new();
    let mut counter = 0u64;
    let trailing_target = bound - kept_from_straddling;
    while trailing.len() + last.len() < trailing_target {
        counter += 1;
        let next = line(counter);
        if trailing.len() + next.len() + last.len() > trailing_target {
            break;
        }
        trailing.push_str(&next);
    }
    // One padding line takes up whatever the whole lines could not.
    let padding = trailing_target - trailing.len() - last.len();
    if padding != 0 {
        trailing.push_str(&"p".repeat(padding - 1));
        trailing.push('\n');
    }
    trailing.push_str(last);
    assert_eq!(trailing.len(), trailing_target);

    let mut stderr = io::stderr().lock();
    let mut lead = String::from("stage:the_first_thing_it_did\n");
    while lead.len() < lead_bytes {
        counter += 1;
        lead.push_str(&line(counter));
    }
    stderr.write_all(lead.as_bytes())?;
    stderr.write_all(STRADDLING.as_bytes())?;
    stderr.write_all(trailing.as_bytes())?;
    stderr.flush()?;

    let total = lead.len() + STRADDLING.len() + trailing.len();
    println!("{} {} {}", total, lead.len(), STRADDLING.len());
    Ok(())
}
