//! A human-readable record of one run: every event delivered to a core,
//! every effect it returned, and the world's own decisions (bucket writes,
//! partitions, crashes). Recording is off during sweeps and on for replays.

pub struct Line {
    pub time: u64,
    pub actor: String,
    pub cell: Option<String>,
    pub text: String,
}

pub struct Trace {
    enabled: bool,
    pub lines: Vec<Line>,
}

impl Trace {
    pub fn new(enabled: bool) -> Self {
        Trace {
            enabled,
            lines: Vec::new(),
        }
    }

    pub fn line(&mut self, time: u64, actor: &str, cell: Option<&str>, text: String) {
        if !self.enabled {
            return;
        }
        self.lines.push(Line {
            time,
            actor: actor.to_string(),
            cell: cell.map(str::to_string),
            text,
        });
    }

    /// The lines that concern `cell`, plus every node- and world-level line
    /// (lease, partition, crash, recovery), which is what explains them.
    pub fn focused(&self, cell: Option<&str>, until: u64) -> Vec<&Line> {
        self.lines
            .iter()
            .filter(|l| l.time <= until)
            .filter(|l| match (cell, l.cell.as_deref()) {
                (Some(want), Some(have)) => want == have,
                (_, None) => !is_noise(&l.text),
                (None, Some(_)) => true,
            })
            .collect()
    }
}

/// Node-level lines that rarely explain a cell's history: load samples and
/// routine timers.
fn is_noise(text: &str) -> bool {
    text.contains("LoadSampled")
        || text.contains("OperationDeadline")
        || text.starts_with("-> ScheduleTimer")
        || text.contains("ActivityFinished")
}

pub fn render(lines: &[&Line], limit: usize) -> String {
    let skip = lines.len().saturating_sub(limit);
    let mut out = String::new();
    if skip > 0 {
        out.push_str(&format!("... {skip} earlier lines omitted ...\n"));
    }
    for l in &lines[skip..] {
        // Long lines are write lists; their tail is the interesting part.
        let text = if l.text.len() > 400 {
            let head = l.text.char_indices().nth(120).map_or(0, |(i, _)| i);
            let tail = l
                .text
                .char_indices()
                .nth(l.text.chars().count() - 240)
                .map_or(0, |(i, _)| i);
            format!("{} ... {}", &l.text[..head], &l.text[tail..])
        } else {
            l.text.clone()
        };
        let actor = match &l.cell {
            Some(cell) if l.actor == "bucket" || l.actor == "client" => {
                format!("{}[{cell}]", l.actor)
            }
            _ => l.actor.clone(),
        };
        out.push_str(&format!("{:>7} {:<10} {}\n", l.time, actor, text));
    }
    out
}
