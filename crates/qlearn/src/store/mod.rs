//! The files of `.learn/`.
//!
//! `.learn/` sits at the top level of the worktree, one directory per
//! course. One server per worktree, and `.learn/` belongs to one worktree,
//! so no lock is needed.

use std::collections::BTreeMap;
use std::fs;
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::SystemTime;

use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::cheatsheet::CheatsheetFile;
use crate::choice::StoredChoosing;
use crate::git::exec::Git;
use crate::model::{self, Block, Course, ImageType, Line, Page, PageKind, Point};
use crate::quiz::StoredQuiz;
use crate::thread::StoredThread;

/// The name of the store, in the worktree and in `.git/info/exclude`.
const DIR: &str = ".learn";

/// The source of the cheatsheet, as the agent wrote it.
pub const CHEATSHEET: &str = "cheatsheet.md";

/// The blocks of the cheatsheet, as the parser wrote them. The excerpts keep
/// the commit that the write read them at, so the source is not parsed
/// again at a read.
const CHEATSHEET_BLOCKS: &str = "cheatsheet.json";

pub struct Store {
    dir: PathBuf,
    /// The names that `qlearn <name>` runs as a command, so a course of
    /// that name could never open.
    commands: Vec<String>,
}

impl Store {
    /// The store of the worktree whose top level is `root`.
    pub fn at(root: &Path) -> Self {
        Self {
            dir: root.join(DIR),
            commands: Vec::new(),
        }
    }

    /// Refuse a slug that is one of `names`: the commands of the binary.
    /// The binary gives them, so a new command is refused with no change
    /// here.
    pub fn refusing(mut self, names: impl IntoIterator<Item = String>) -> Self {
        self.commands = names.into_iter().collect();
        self
    }

    /// `.learn/` itself.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// The directory of a course.
    pub fn course_dir(&self, slug: &str) -> PathBuf {
        self.dir.join(slug)
    }

    /// Open a course, as `qlearn [<slug>]` does.
    ///
    /// With a slug, the course is created when it does not exist. Without
    /// one, `.learn/` must hold exactly one course. A course that cannot be
    /// read is an error, and is never written over.
    pub fn open(&self, slug: Option<&str>, now: OffsetDateTime) -> Result<Course> {
        let slug = match slug {
            Some(slug) => {
                self.check_slug(slug)?;
                slug.to_owned()
            }
            None => self.only_slug()?,
        };

        if let Some(course) = self.load(&slug)? {
            return Ok(course);
        }
        // Only a new course: one made before its slug became a command
        // still opens with `qlearn -- <slug>`.
        self.check_command(&slug)?;
        let course = Course::new(&slug, now);
        self.save(&course)?;

        Ok(course)
    }

    /// The only course of `.learn/`, as `qlearn` with no slug finds it, and
    /// never a new one: this writes nothing.
    pub fn only(&self) -> Result<Course> {
        let slug = self.only_slug()?;
        self.load(&slug)?
            .with_context(|| format!("the course {slug} went away while qlearn read it"))
    }

    fn only_slug(&self) -> Result<String> {
        let mut slugs = self.slugs()?;
        match slugs.len() {
            0 => bail!("{DIR}/ holds no course yet. Give it a name: `qlearn <slug>`"),
            1 => Ok(slugs.remove(0)),
            _ => bail!(
                "{DIR}/ holds several courses: {}. Name one: `qlearn <slug>`",
                slugs.join(", ")
            ),
        }
    }

    /// The slugs of the courses of `.learn/`, in order. A directory
    /// without `course.json` is not a course, so it is left out.
    pub fn slugs(&self) -> Result<Vec<String>> {
        let entries = match fs::read_dir(&self.dir) {
            Ok(entries) => entries,
            Err(e) if e.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => {
                return Err(e).with_context(|| format!("cannot read {}", self.dir.display()));
            }
        };

        let mut slugs: Vec<_> = entries
            .filter_map(|entry| {
                let entry = entry.ok()?;
                if !entry.file_type().ok()?.is_dir() {
                    return None;
                }
                let name = entry.file_name().into_string().ok()?;
                (is_slug(&name) && entry.path().join("course.json").exists()).then_some(name)
            })
            .collect();

        slugs.sort();
        Ok(slugs)
    }

    /// Read a course. None when it has no directory yet.
    pub fn load(&self, slug: &str) -> Result<Option<Course>> {
        let dir = self.course_dir(slug);
        let path = dir.join("course.json");

        match self.find(slug)? {
            Found::Course(course) => Ok(Some(*course)),
            Found::Nothing => Ok(None),
            Found::Foreign => bail!(
                "{} has no course.json, so it is not a course of qlearn. \
                 Start a new course with a new slug: `qlearn <slug>`",
                dir.display()
            ),
            Found::Link => bail!(
                "{} is a symbolic link, and qlearn writes under {DIR}/ only. \
                 Start a new course with a new slug: `qlearn <slug>`",
                dir.display()
            ),
            Found::Newer(version) => bail!(
                "{} was written by a newer qlearn (format {version}, this one \
                 reads {}). Upgrade qlearn",
                path.display(),
                model::VERSION
            ),
            Found::Corrupt(e) => Err(e).with_context(|| {
                format!(
                    "{} is not readable. It is left as it is: repair it by hand \
                     rather than lose the course",
                    path.display()
                )
            }),
        }
    }

    /// What the directory of a course holds. `load()` turns it into an
    /// error, `listing()` into a line, so each case keeps its own advice.
    fn find(&self, slug: &str) -> Result<Found> {
        let dir = self.course_dir(slug);
        let path = dir.join("course.json");

        // A link would take every write of the course out of `.learn/`.
        if fs::symlink_metadata(&dir).is_ok_and(|meta| meta.file_type().is_symlink()) {
            return Ok(Found::Link);
        }
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(e) if e.kind() == ErrorKind::NotFound => {
                // Another tool may have written the directory, and its
                // files are not a contract: they are never read or written.
                if dir.exists() {
                    return Ok(Found::Foreign);
                }
                return Ok(Found::Nothing);
            }
            Err(e) => return Err(e).with_context(|| format!("cannot read {}", path.display())),
        };

        // The version is read alone first: a newer format may not parse as
        // this one, and the learner must be told to upgrade, not to repair.
        #[derive(Deserialize)]
        struct Head {
            version: u32,
        }
        let head: Head = match serde_json::from_str(&text) {
            Ok(head) => head,
            Err(e) => return Ok(Found::Corrupt(e)),
        };
        if head.version > model::VERSION {
            return Ok(Found::Newer(head.version));
        }

        Ok(match serde_json::from_str::<Course>(&text) {
            // The directory names the course, and every write goes back to
            // it. A copied directory is a course of its own.
            Ok(mut course) => {
                course.slug = slug.to_owned();
                Found::Course(Box::new(course))
            }
            Err(e) => Found::Corrupt(e),
        })
    }

    /// Write `course.json`. The answer puts the file back as it was.
    pub fn save_undoable(&self, course: &Course) -> Result<Undo> {
        let path = self.course_dir(&course.slug).join("course.json");
        let undo = Undo {
            files: vec![(path.clone(), read_if_any(&path)?)],
        };
        self.save(course)?;
        Ok(undo)
    }

    pub fn save(&self, course: &Course) -> Result<()> {
        let mut text = serde_json::to_string_pretty(course)?;
        text.push('\n');

        write_atomic(
            &self.course_dir(&course.slug).join("course.json"),
            text.as_bytes(),
        )
    }

    /// The courses as `qlearn list` prints them.
    pub fn listing(&self) -> Result<String> {
        let slugs = self.slugs()?;
        if slugs.is_empty() {
            return Ok(format!("{DIR}/ holds no course yet.\n"));
        }

        let mut out = String::new();
        for slug in slugs {
            // A course of a newer qlearn is not to be repaired, so it is
            // not called unreadable.
            let line = match self.find(&slug) {
                Ok(Found::Course(course)) => {
                    format!("{}  {}", course.status.as_str(), course.title)
                }
                // Its course.json went away since `slugs()`.
                Ok(Found::Nothing | Found::Foreign | Found::Link) => continue,
                Ok(Found::Newer(_)) => "(newer qlearn)".to_owned(),
                Ok(Found::Corrupt(_)) | Err(_) => "(unreadable)".to_owned(),
            };
            out.push_str(&format!("{slug}  {line}\n"));
        }
        Ok(out)
    }
}

/// What `pages/<id>.json` holds.
///
/// Each field takes `serde(default)`, so the file of an earlier release
/// still reads.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", default)]
pub struct PageFile {
    /// The hidden line of an excerpt is None here too, so a route that
    /// sends the blocks as stored gives nothing away.
    pub blocks: Vec<Block>,
    /// The text of each hidden line, by the id of its excerpt.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub hidden: BTreeMap<String, Line>,
    /// In the order that the agent added them, each with its right
    /// options. A push of the blocks keeps them.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub quizzes: Vec<StoredQuiz>,
    /// In the order that they opened. A push of the blocks keeps them, so
    /// a thread whose block is gone is kept too.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub threads: Vec<StoredThread>,
}

impl PageFile {
    /// The page as the page of the learner may see it, with the choices of
    /// the course at its end. Every route that sends a page goes through
    /// here, so none of them drops an open choice.
    pub fn page(&self, id: &str, choices: &[StoredChoosing]) -> Page {
        let mut quizzes: Vec<_> = self.quizzes.iter().map(StoredQuiz::view).collect();
        if !self.answered() {
            for debrief in quizzes.iter_mut().filter_map(|quiz| quiz.debrief.as_mut()) {
                self.hide_in(&mut debrief.blocks);
            }
        }
        Page {
            id: id.to_owned(),
            blocks: self.shown(),
            quizzes,
            threads: self
                .threads()
                .into_iter()
                .map(|(thread, _)| thread.view(id, crate::lesson::markdown))
                .collect(),
            choices: choices
                .iter()
                .filter(|choice| choice.page.as_deref() == Some(id))
                .map(StoredChoosing::view)
                .collect(),
        }
    }

    /// The threads in the order that the page shows them, each with
    /// whether its block is gone. A thread whose block a later push took
    /// away moves to the end, with its quote (section 6 of the design).
    ///
    /// The blocks decide at each read, so a block that a later push brings
    /// back takes its threads back.
    pub fn threads(&self) -> Vec<(&StoredThread, bool)> {
        let (held, gone): (Vec<_>, Vec<_>) = self
            .threads
            .iter()
            .map(|thread| (thread, self.lost(thread)))
            .partition(|(_, gone)| !gone);
        held.into_iter().chain(gone).collect()
    }

    /// Whether the block of a thread is gone from the page and from the
    /// debriefs of its quizzes.
    pub fn lost(&self, thread: &StoredThread) -> bool {
        thread
            .block
            .as_ref()
            .is_some_and(|block| self.block(&block.id).is_none())
    }

    /// Put None in place of each line of `blocks` that holds the text of a
    /// hidden line of the lesson.
    ///
    /// A debrief shows once its own quiz is answered, while another quiz of
    /// the page may still wait, and a quiz pushed later makes the page wait
    /// again. Its excerpts may show the hidden line, at its place or at
    /// another one, so the text decides. The cheatsheet goes through here
    /// for each page that waits for an answer.
    pub fn hide_in(&self, blocks: &mut [Block]) {
        let hidden: Vec<&str> = self.hidden.values().map(|line| line.text.trim()).collect();
        for block in blocks {
            let Block::Excerpt { excerpt, .. } = block else {
                continue;
            };
            for line in &mut excerpt.lines {
                if line
                    .as_ref()
                    .is_some_and(|line| hidden.contains(&line.text.trim()))
                {
                    *line = None;
                }
            }
        }
    }

    /// The blocks as the page may see them.
    ///
    /// A hidden line stays None until the learner answers every quiz of the
    /// page. It stays None too while the page has no quiz, because the
    /// agent pushes the quiz after the page.
    pub fn shown(&self) -> Vec<Block> {
        match self.answered() {
            true => self.revealed(),
            false => self.blocks.clone(),
        }
    }

    /// Whether the learner answered every quiz of the page.
    pub fn answered(&self) -> bool {
        !self.quizzes.is_empty() && self.quizzes.iter().all(StoredQuiz::answered)
    }

    /// Whether the agent wrote the debrief of every quiz of the page.
    pub fn debriefed(&self) -> bool {
        !self.quizzes.is_empty() && self.quizzes.iter().all(|quiz| quiz.debrief.is_some())
    }

    /// The score of the page, or None while nothing of it has a result.
    ///
    /// Each choice and each point of an open question counts 1 once it has
    /// a result. A right one adds 1, a partial one 0.5 (section 5.3 of the
    /// design).
    pub fn score(&self) -> Option<model::Score> {
        let mut right = 0.0;
        let mut total = 0;
        for quiz in &self.quizzes {
            let grades = quiz.grades().into_iter().map(|(_, grade)| grade);
            let open = quiz.debrief.iter().flat_map(|debrief| &debrief.points);
            for grade in grades.chain(open.map(|graded| graded.result)) {
                total += 1;
                right += match grade {
                    model::Grade::Ok => 1.0,
                    model::Grade::Partial => 0.5,
                    model::Grade::Missed => 0.0,
                };
            }
        }
        (total > 0).then_some(model::Score { right, total })
    }

    /// A block of the page, or of the debrief of one of its quizzes, with
    /// the number of that quiz.
    pub fn block(&self, id: &str) -> Option<(&Block, Option<usize>)> {
        let lesson = self.blocks.iter().map(|block| (block, None));
        let debriefs = self.quizzes.iter().enumerate().flat_map(|(at, quiz)| {
            let blocks = quiz.debrief.iter().flat_map(|debrief| &debrief.blocks);
            blocks.map(move |block| (block, Some(at + 1)))
        });
        lesson.chain(debriefs).find(|(block, _)| block.id() == id)
    }

    /// The blocks with the text of each hidden line.
    fn revealed(&self) -> Vec<Block> {
        let mut blocks = self.blocks.clone();
        for block in &mut blocks {
            let Block::Excerpt { id, excerpt } = block else {
                continue;
            };
            let Some(line) = self.hidden.get(id.as_str()) else {
                continue;
            };
            if let Some(hidden) = excerpt.lines.iter_mut().find(|line| line.is_none()) {
                *hidden = Some(line.clone());
            }
        }
        blocks
    }
}

/// What `choices.json` holds. See section 7 of the design.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", default)]
struct ChoicesFile {
    /// In the order that the agent asked them. A choice is never removed.
    choices: Vec<StoredChoosing>,
}

/// What `points.json` holds. See section 5.3 of the design.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", default)]
struct PointsFile {
    points: Vec<Point>,
}

impl Store {
    /// The directory of the pages of a course.
    fn pages_dir(&self, slug: &str) -> Result<PathBuf> {
        self.inner_dir(slug, "pages")
    }

    /// The directory of the images of a course.
    fn media_dir(&self, slug: &str) -> Result<PathBuf> {
        self.inner_dir(slug, "media")
    }

    /// A directory of a course, which must not be a link.
    fn inner_dir(&self, slug: &str, name: &str) -> Result<PathBuf> {
        let dir = self.course_dir(slug).join(name);
        // A link would take every write in it out of `.learn/`.
        if fs::symlink_metadata(&dir).is_ok_and(|meta| meta.file_type().is_symlink()) {
            bail!(
                "{} is a symbolic link, and qlearn writes under {DIR}/ only. \
                 Put a directory in its place",
                dir.display()
            );
        }
        Ok(dir)
    }

    /// Write a page: its source as the agent pushed it, and its blocks.
    ///
    /// The source is kept as it came, so a later release can parse it again.
    /// The answer puts the files back as they were, for a push that fails
    /// after this write. A write that fails here puts them back itself.
    pub fn save_page(&self, slug: &str, id: &str, source: &str, page: &PageFile) -> Result<Undo> {
        check_page_id(id)?;
        let dir = self.pages_dir(slug)?;
        let mut text = serde_json::to_string_pretty(page)?;
        text.push('\n');

        let source_path = dir.join(format!("{id}.md"));
        let blocks_path = dir.join(format!("{id}.json"));
        let undo = Undo {
            files: vec![
                (source_path.clone(), read_if_any(&source_path)?),
                (blocks_path.clone(), read_if_any(&blocks_path)?),
            ],
        };

        let written = write_atomic(&source_path, source.as_bytes())
            .and_then(|()| write_atomic(&blocks_path, text.as_bytes()));
        match written {
            Ok(()) => Ok(undo),
            Err(error) => Err(undo.after(error)),
        }
    }

    /// Write the images of a page into `media/`, each under the hash of its
    /// bytes and the extension of its type.
    ///
    /// A name that is there already holds the same bytes, so it is never
    /// written twice. The answer removes the files that this write added,
    /// for a push that fails after it.
    pub fn save_media(&self, slug: &str, media: &BTreeMap<String, Vec<u8>>) -> Result<Undo> {
        let mut undo = Undo { files: Vec::new() };
        if media.is_empty() {
            return Ok(undo);
        }
        let dir = self.media_dir(slug)?;
        for (name, bytes) in media {
            if ImageType::of_media(name).is_none() {
                return Err(undo.after(anyhow!("`{name}` is not a name of {DIR}/{slug}/media")));
            }
            let path = dir.join(name);
            if path.exists() {
                continue;
            }
            if let Err(error) = write_atomic(&path, bytes) {
                return Err(undo.after(error));
            }
            undo.files.push((path, None));
        }
        Ok(undo)
    }

    /// The bytes of an image of `media/`. None when the name is not the
    /// name of an image, or names no file.
    pub fn load_media(&self, slug: &str, name: &str) -> Result<Option<Vec<u8>>> {
        if ImageType::of_media(name).is_none() {
            return Ok(None);
        }
        let path = self.media_dir(slug)?.join(name);
        match fs::read(&path) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(e) if e.kind() == ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e).with_context(|| format!("cannot read {}", path.display())),
        }
    }

    /// Write the blocks of a page, and leave its source as it is.
    ///
    /// A quiz goes through here. Only a push of the page writes the source,
    /// so the current page stays the page pushed last. The answer puts the
    /// file back as it was.
    pub fn save_blocks(&self, slug: &str, id: &str, page: &PageFile) -> Result<Undo> {
        check_page_id(id)?;
        let path = self.pages_dir(slug)?.join(format!("{id}.json"));
        let mut text = serde_json::to_string_pretty(page)?;
        text.push('\n');

        let undo = Undo {
            files: vec![(path.clone(), read_if_any(&path)?)],
        };
        write_atomic(&path, text.as_bytes())?;
        Ok(undo)
    }

    /// The points of a course. None of them before the first quiz.
    ///
    /// A file that cannot be read is an error, and is never written over:
    /// it holds the results of the learner.
    pub fn load_points(&self, slug: &str) -> Result<Vec<Point>> {
        let path = self.course_dir(slug).join("points.json");
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(e) if e.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e).with_context(|| format!("cannot read {}", path.display())),
        };

        let file: PointsFile = serde_json::from_str(&text).with_context(|| {
            format!(
                "{} is not readable. It is left as it is: repair it by hand rather than \
                 lose the results of the learner",
                path.display()
            )
        })?;
        Ok(file.points)
    }

    /// Write the points of a course. The answer puts the file back as it
    /// was.
    pub fn save_points(&self, slug: &str, points: &[Point]) -> Result<Undo> {
        let path = self.course_dir(slug).join("points.json");
        let file = PointsFile {
            points: points.to_vec(),
        };
        let mut text = serde_json::to_string_pretty(&file)?;
        text.push('\n');

        let undo = Undo {
            files: vec![(path.clone(), read_if_any(&path)?)],
        };
        write_atomic(&path, text.as_bytes())?;
        Ok(undo)
    }

    /// The choices of a course. None of them before the first one.
    ///
    /// A file that cannot be read is an error, and is never written over:
    /// it holds the answers of the learner.
    pub fn load_choices(&self, slug: &str) -> Result<Vec<StoredChoosing>> {
        let path = self.course_dir(slug).join("choices.json");
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(e) if e.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e).with_context(|| format!("cannot read {}", path.display())),
        };

        let file: ChoicesFile = serde_json::from_str(&text).with_context(|| {
            format!(
                "{} is not readable. It is left as it is: repair it by hand rather than \
                 lose the answers of the learner",
                path.display()
            )
        })?;
        Ok(file.choices)
    }

    /// Write the choices of a course.
    pub fn save_choices(&self, slug: &str, choices: &[StoredChoosing]) -> Result<()> {
        let file = ChoicesFile {
            choices: choices.to_vec(),
        };
        let mut text = serde_json::to_string_pretty(&file)?;
        text.push('\n');

        write_atomic(&self.course_dir(slug).join("choices.json"), text.as_bytes())
    }

    /// Read the blocks of a page. None when the agent has not pushed it.
    ///
    /// A file that cannot be read is an error, and is never written over:
    /// it holds the quizzes of the page and the answers of the learner. A
    /// push of the page reads it first, so a push cannot repair it.
    pub fn load_page(&self, slug: &str, id: &str) -> Result<Option<PageFile>> {
        check_page_id(id)?;
        let path = self.pages_dir(slug)?.join(format!("{id}.json"));
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(e) if e.kind() == ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e).with_context(|| format!("cannot read {}", path.display())),
        };

        serde_json::from_str(&text).map(Some).with_context(|| {
            format!(
                "{} is not readable. It is left as it is: repair it by hand rather \
                 than lose the quizzes of the page and the answers of the learner",
                path.display()
            )
        })
    }

    /// Write the cheatsheet: its source as the agent wrote it, then its
    /// blocks. The blocks go last, so a cheatsheet with no blocks file is
    /// one whose write failed, and reads as not written.
    ///
    /// The answer puts the files back as they were, for a write that fails
    /// after this one. A write that fails here puts them back itself.
    pub fn save_cheatsheet(
        &self,
        slug: &str,
        source: &str,
        sheet: &CheatsheetFile,
    ) -> Result<Undo> {
        let dir = self.course_dir(slug);
        let mut text = serde_json::to_string_pretty(sheet)?;
        text.push('\n');

        let source_path = dir.join(CHEATSHEET);
        let blocks_path = dir.join(CHEATSHEET_BLOCKS);
        let undo = Undo {
            files: vec![
                (source_path.clone(), read_if_any(&source_path)?),
                (blocks_path.clone(), read_if_any(&blocks_path)?),
            ],
        };

        let written = write_atomic(&source_path, source.as_bytes())
            .and_then(|()| write_atomic(&blocks_path, text.as_bytes()));
        match written {
            Ok(()) => Ok(undo),
            Err(error) => Err(undo.after(error)),
        }
    }

    /// The blocks of the cheatsheet. None until the agent writes it.
    ///
    /// The file holds nothing of the learner, so a file that cannot be read
    /// is an error that a new write of the agent repairs.
    pub fn load_cheatsheet(&self, slug: &str) -> Result<Option<CheatsheetFile>> {
        let path = self.course_dir(slug).join(CHEATSHEET_BLOCKS);
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(e) if e.kind() == ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e).with_context(|| format!("cannot read {}", path.display())),
        };

        serde_json::from_str(&text).map(Some).with_context(|| {
            format!(
                "{} is not readable. Ask the agent to write the cheatsheet again with \
                 `qlearn cheatsheet`",
                path.display()
            )
        })
    }

    /// The source of a page, as the agent pushed it. None when the agent
    /// has not pushed it.
    pub fn load_source(&self, slug: &str, id: &str) -> Result<Option<String>> {
        check_page_id(id)?;
        let path = self.pages_dir(slug)?.join(format!("{id}.md"));
        match fs::read_to_string(&path) {
            Ok(text) => Ok(Some(text)),
            Err(e) if e.kind() == ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e).with_context(|| format!("cannot read {}", path.display())),
        }
    }

    /// The page that the agent pushed last, or None before the first push.
    ///
    /// Only a push writes `pages/<id>.md`, so the newest source is the last
    /// push. The `.json` would not do: an answer of the learner rewrites
    /// it. A server that starts again opens the page where the course
    /// stopped, and `course.json` needs no field for it.
    pub fn current(&self, course: &Course) -> Result<Option<String>> {
        let dir = self.pages_dir(&course.slug)?;
        let entries = match fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(e) if e.kind() == ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e).with_context(|| format!("cannot read {}", dir.display())),
        };

        let mut newest: Option<(SystemTime, String)> = None;
        for entry in entries.flatten() {
            let Ok(name) = entry.file_name().into_string() else {
                continue;
            };
            // A temporary file of a write ends in `.tmp`, so it never counts.
            let Some(id) = name.strip_suffix(".md") else {
                continue;
            };
            if !course.has_content(id) {
                continue;
            }
            let Ok(modified) = entry.metadata().and_then(|meta| meta.modified()) else {
                continue;
            };
            if newest.as_ref().is_none_or(|(at, _)| modified > *at) {
                newest = Some((modified, id.to_owned()));
            }
        }
        Ok(newest.map(|(_, id)| id))
    }
}

/// What `qlearn delete` removes: the directory of a course, and what the
/// learner loses with it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Removal {
    pub slug: String,
    pub dir: PathBuf,
    /// None when `course.json` cannot be read: the course goes all the same.
    pub title: Option<String>,
    /// The pages that the agent pushed.
    pub pages: usize,
    pub cheatsheet: bool,
    /// The files under `notes/`.
    pub notes: usize,
}

impl Store {
    /// The files under `notes/` of a course, relative to it, in order.
    ///
    /// The agent writes them with its own tools. Only their names are read,
    /// and a link is never followed: it may lead out of the store.
    pub fn notes(&self, slug: &str) -> Result<Vec<String>> {
        let root = self.course_dir(slug).join("notes");
        let mut out = Vec::new();
        if !fs::symlink_metadata(&root).is_ok_and(|meta| meta.is_dir()) {
            return Ok(out);
        }
        let mut todo = vec![root.clone()];
        while let Some(dir) = todo.pop() {
            let entries =
                fs::read_dir(&dir).with_context(|| format!("cannot read {}", dir.display()))?;
            for entry in entries {
                let entry = entry.with_context(|| format!("cannot read {}", dir.display()))?;
                let path = entry.path();
                // The type of the entry itself: a link to a directory is a
                // name, not a directory to walk.
                match entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                    true => todo.push(path),
                    false => out.push(
                        path.strip_prefix(&root)
                            .unwrap_or(&path)
                            .to_string_lossy()
                            .into_owned(),
                    ),
                }
            }
        }
        out.sort();
        Ok(out)
    }

    /// What `qlearn delete <slug>` would remove, or why it refuses.
    ///
    /// Only a course of qlearn goes: a directory without `course.json`
    /// belongs to another tool, and a link may lead out of `.learn/`. A
    /// course that cannot be read goes too, since the learner may want
    /// exactly that.
    pub fn removal(&self, slug: &str) -> Result<Removal> {
        self.check_slug(slug)?;
        let dir = self.course_dir(slug);
        let title = match self.find(slug) {
            Ok(Found::Course(course)) => Some(course.title),
            Ok(Found::Newer(_) | Found::Corrupt(_)) => None,
            // `find` fails on the read of `course.json` alone, and a course
            // whose `course.json` cannot be read goes all the same.
            Err(_) if fs::symlink_metadata(&dir).is_ok_and(|meta| meta.is_dir()) => None,
            Err(error) => return Err(error),
            Ok(Found::Nothing) => {
                let slugs = self.slugs()?;
                match slugs.is_empty() {
                    true => bail!("{DIR}/ holds no course yet, so no course {slug}"),
                    false => bail!(
                        "{DIR}/ holds no course {slug}. Its courses: {}",
                        slugs.join(", ")
                    ),
                }
            }
            Ok(Found::Foreign) => bail!(
                "{} has no course.json, so it is not a course of qlearn. qlearn deletes \
                 only its own courses",
                dir.display()
            ),
            Ok(Found::Link) => bail!(
                "{} is a symbolic link, not a course of qlearn. qlearn deletes only its \
                 own courses",
                dir.display()
            ),
        };

        let pages = dir.join("pages");
        let pages = match fs::symlink_metadata(&pages).is_ok_and(|meta| meta.is_dir()) {
            true => fs::read_dir(&pages)
                .with_context(|| format!("cannot read {}", pages.display()))?
                .filter_map(|entry| entry.ok()?.file_name().into_string().ok())
                .filter(|name| {
                    name.strip_suffix(".md")
                        .is_some_and(|id| check_page_id(id).is_ok())
                })
                .count(),
            false => 0,
        };
        Ok(Removal {
            slug: slug.to_owned(),
            title,
            pages,
            // The blocks file, as a read: a source alone is a write that failed.
            cheatsheet: dir.join(CHEATSHEET_BLOCKS).exists(),
            notes: self.notes(slug)?.len(),
            dir,
        })
    }

    /// Remove the directory of a course, and nothing else.
    ///
    /// The removal does not follow a link inside the course: it removes the
    /// link, and leaves what it leads to.
    pub fn remove(&self, removal: &Removal) -> Result<()> {
        // `removal()` refused a link in place of the course.
        fs::remove_dir_all(&removal.dir)
            .with_context(|| format!("cannot remove {}", removal.dir.display()))
    }
}

/// The files of a write before it, so that a change that fails halfway
/// leaves the store as it was.
#[derive(Debug)]
pub struct Undo {
    /// Each file, with its bytes before the write, or None when it did not
    /// exist.
    files: Vec<(PathBuf, Option<Vec<u8>>)>,
}

impl Undo {
    /// One undo for this write and a later one. The later write is put
    /// back first, so a file that both wrote ends as it was before both.
    pub fn and(self, later: Undo) -> Undo {
        let mut files = later.files;
        files.extend(self.files);
        Undo { files }
    }

    /// Put the files back as they were.
    pub fn undo(self) -> Result<()> {
        for (path, before) in self.files {
            match before {
                Some(bytes) => write_atomic(&path, &bytes)?,
                None => match fs::remove_file(&path) {
                    Ok(()) => {}
                    Err(e) if e.kind() == ErrorKind::NotFound => {}
                    Err(e) => {
                        return Err(e).with_context(|| format!("cannot remove {}", path.display()));
                    }
                },
            }
        }
        Ok(())
    }

    /// Put the files back after `error`, and give the error to answer.
    pub fn after(self, error: anyhow::Error) -> anyhow::Error {
        match self.undo() {
            Ok(()) => error,
            Err(undo) => anyhow!("{error:#}, and the store could not be put back: {undo:#}"),
        }
    }
}

/// The bytes of a file, or None when it does not exist.
fn read_if_any(path: &Path) -> Result<Option<Vec<u8>>> {
    match fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e).with_context(|| format!("cannot read {}", path.display())),
    }
}

/// The kind of a page id, or the error that the agent reads.
///
/// A page id names two files of the store, so it must never reach outside
/// `pages/`.
pub fn check_page_id(id: &str) -> Result<PageKind> {
    match PageKind::of_id(id) {
        Some(kind) => Ok(kind),
        None => bail!(
            "`{id}` is not a page id. Use a number with an optional letter, such as 7 \
             or 7a, or c1 for a checkpoint, or f1 for a follow-up"
        ),
    }
}

/// What `Store::find()` sees in the directory of a course.
enum Found {
    Course(Box<Course>),
    /// No directory: the course is new.
    Nothing,
    /// A directory without `course.json`: not a course of qlearn.
    Foreign,
    /// A symbolic link in place of the directory.
    Link,
    /// A format above `model::VERSION`.
    Newer(u32),
    Corrupt(serde_json::Error),
}

/// Write a file of the store.
///
/// The write is atomic: a full file appears, or the old one stays. A
/// half-written course is worse than none, because nobody notices.
fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let dir = path
        .parent()
        .context("a file of the store has a directory")?;
    fs::create_dir_all(dir).with_context(|| format!("cannot make {}", dir.display()))?;

    // A name of its own for each write: two writers of one file must not
    // share a temporary file, or one truncates what the other wrote.
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let mut temp = path.as_os_str().to_owned();
    temp.push(format!(
        ".{}.{}.tmp",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let temp = PathBuf::from(temp);

    let written = fs::File::create(&temp)
        .and_then(|mut handle| {
            handle.write_all(bytes)?;
            handle.sync_all()
        })
        .with_context(|| format!("cannot write {}", temp.display()))
        .and_then(|()| {
            fs::rename(&temp, path)
                .with_context(|| format!("cannot put {} in place", path.display()))
        });
    if written.is_err() {
        let _ = fs::remove_file(&temp);
    }
    written
}

/// A slug names a directory, so it must never reach outside `.learn/`.
fn is_slug(slug: &str) -> bool {
    !slug.is_empty()
        && slug
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

impl Store {
    fn check_slug(&self, slug: &str) -> Result<()> {
        if !is_slug(slug) {
            bail!("`{slug}` is not a slug. Use lowercase letters, digits and hyphens");
        }
        Ok(())
    }

    fn check_command(&self, slug: &str) -> Result<()> {
        if self.commands.iter().any(|name| name == slug) {
            bail!(
                "`{slug}` is a command of qlearn: `qlearn {slug}` runs it, so this course \
                 could never open. Pick another slug"
            );
        }
        Ok(())
    }
}

/// Make git ignore `.learn/`, so a course is never committed by accident.
///
/// When git does not ignore it yet, the line goes into `info/exclude`,
/// which no commit carries. This is the one write outside `.learn/`.
pub async fn exclude(git: &Git) -> Result<()> {
    let pattern = format!("{DIR}/");
    let (ignored, _) = git.output(&["check-ignore", "-q", &pattern]).await?;
    if ignored {
        return Ok(());
    }

    // A linked worktree has no `.git` directory: git names the file.
    let path = git
        .text(&["rev-parse", "--git-path", "info/exclude"])
        .await?;
    let path = git.root().join(path.trim());

    // The file is the user's: an append never loses their lines, even
    // when the write fails half way.
    let before = match fs::read(&path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == ErrorKind::NotFound => Vec::new(),
        Err(e) => return Err(e).with_context(|| format!("cannot read {}", path.display())),
    };
    // git still says no when a .gitignore un-ignores the store, or when
    // the store is tracked. The line cannot change that, so one is enough.
    if String::from_utf8_lossy(&before)
        .lines()
        .any(|line| line.trim() == pattern)
    {
        return Ok(());
    }
    let last = before.last().copied();
    let mut line = String::new();
    if last.is_some_and(|b| b != b'\n') {
        line.push('\n');
    }
    line.push_str(&pattern);
    line.push('\n');

    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).with_context(|| format!("cannot make {}", dir.display()))?;
    }
    fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .and_then(|mut file| file.write_all(line.as_bytes()))
        .with_context(|| format!("cannot write {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::CourseStatus;
    use crate::testutil::{Repo, build_repo, commit};

    fn at_new_year() -> OffsetDateTime {
        OffsetDateTime::from_unix_timestamp(1_767_225_600).unwrap()
    }

    async fn repo() -> Repo {
        build_repo(&[commit("first").file("a.txt", "a\n")]).await
    }

    fn write(path: &Path, text: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    /// Every file under `dir`, relative to it, `.git` left out.
    fn files(dir: &Path) -> Vec<String> {
        fn walk(base: &Path, dir: &Path, out: &mut Vec<String>) {
            for entry in fs::read_dir(dir).unwrap() {
                let path = entry.unwrap().path();
                let name = path
                    .strip_prefix(base)
                    .unwrap()
                    .to_string_lossy()
                    .into_owned();
                if name == ".git" {
                    continue;
                }
                if path.is_dir() {
                    walk(base, &path, out);
                } else {
                    out.push(name);
                }
            }
        }
        let mut out = Vec::new();
        walk(dir, dir, &mut out);
        out.sort();
        out
    }

    #[tokio::test]
    async fn a_new_course_is_written_and_read_back() {
        let repo = repo().await;
        let store = Store::at(repo.path());

        let course = store.open(Some("raft"), at_new_year()).unwrap();
        assert_eq!(course.title, "raft");
        assert_eq!(course.status, CourseStatus::New);
        assert_eq!(course.created, "2026-01-01T00:00:00Z");
        assert!(repo.path().join(".learn/raft/course.json").is_file());

        let mut changed = course.clone();
        changed.title = "Raft elections".to_owned();
        store.save(&changed).unwrap();

        let later = OffsetDateTime::from_unix_timestamp(1_800_000_000).unwrap();
        let back = store.open(Some("raft"), later).unwrap();
        assert_eq!(back, changed, "an open course is read, not created again");
    }

    #[tokio::test]
    async fn a_write_leaves_no_temporary_file_behind() {
        let repo = repo().await;
        let store = Store::at(repo.path());
        store.open(Some("raft"), at_new_year()).unwrap();

        assert_eq!(files(repo.path()), [".learn/raft/course.json", "a.txt"]);
    }

    #[tokio::test]
    async fn a_newer_format_is_refused_with_a_word_to_upgrade() {
        let repo = repo().await;
        let path = repo.path().join(".learn/raft/course.json");
        // A shape that this qlearn cannot parse: the version alone decides.
        let text = r#"{"version":2,"slug":"raft","parts":"elsewhere"}"#;
        write(&path, text);

        let error = Store::at(repo.path())
            .open(Some("raft"), at_new_year())
            .unwrap_err()
            .to_string();

        assert!(error.contains("newer qlearn"), "{error}");
        assert!(error.contains("Upgrade qlearn"), "{error}");
        assert_eq!(fs::read_to_string(&path).unwrap(), text);
    }

    #[tokio::test]
    async fn a_directory_without_a_course_file_is_refused_and_left_alone() {
        let repo = repo().await;
        let store = Store::at(repo.path());
        write(&repo.path().join(".learn/raft/notes.txt"), "elections\n");

        let error = store
            .open(Some("raft"), at_new_year())
            .unwrap_err()
            .to_string();

        assert!(error.contains("not a course of qlearn"), "{error}");
        assert!(error.contains("new course with a new slug"), "{error}");
        assert_eq!(files(repo.path()), [".learn/raft/notes.txt", "a.txt"]);
        assert_eq!(
            fs::read_to_string(repo.path().join(".learn/raft/notes.txt")).unwrap(),
            "elections\n"
        );
    }

    #[tokio::test]
    async fn a_directory_without_a_course_file_is_skipped() {
        let repo = repo().await;
        let store = Store::at(repo.path());
        write(&repo.path().join(".learn/bully/notes.txt"), "bully\n");
        fs::create_dir_all(repo.path().join(".learn/empty")).unwrap();

        assert_eq!(store.listing().unwrap(), ".learn/ holds no course yet.\n");
        let error = store.open(None, at_new_year()).unwrap_err().to_string();
        assert!(error.contains("holds no course yet"), "{error}");

        store.open(Some("raft"), at_new_year()).unwrap();
        assert_eq!(store.open(None, at_new_year()).unwrap().slug, "raft");
        assert_eq!(store.listing().unwrap(), "raft  new  raft\n");
        assert_eq!(
            files(repo.path()),
            [".learn/bully/notes.txt", ".learn/raft/course.json", "a.txt"]
        );
    }

    #[tokio::test]
    async fn a_link_in_place_of_a_course_is_refused_and_left_alone() {
        let repo = repo().await;
        let store = Store::at(repo.path());
        let elsewhere = tempfile::tempdir().unwrap();
        fs::create_dir_all(repo.path().join(".learn")).unwrap();
        std::os::unix::fs::symlink(elsewhere.path(), repo.path().join(".learn/raft")).unwrap();

        let error = store
            .open(Some("raft"), at_new_year())
            .unwrap_err()
            .to_string();

        assert!(error.contains("symbolic link"), "{error}");
        assert!(error.contains("new course with a new slug"), "{error}");
        assert_eq!(files(elsewhere.path()), Vec::<String>::new());
        assert_eq!(store.listing().unwrap(), ".learn/ holds no course yet.\n");
    }

    #[tokio::test]
    async fn a_copied_course_is_named_after_its_directory() {
        let repo = repo().await;
        let store = Store::at(repo.path());
        store.open(Some("raft"), at_new_year()).unwrap();
        let original = repo.path().join(".learn/raft/course.json");
        let text = fs::read_to_string(&original).unwrap();
        write(&repo.path().join(".learn/raft-2/course.json"), &text);

        let mut copy = store.open(Some("raft-2"), at_new_year()).unwrap();
        assert_eq!(copy.slug, "raft-2");
        copy.title = "The copy".to_owned();
        store.save(&copy).unwrap();

        assert_eq!(fs::read_to_string(&original).unwrap(), text);
        assert_eq!(store.open(Some("raft-2"), at_new_year()).unwrap(), copy);
    }

    #[tokio::test]
    async fn two_writers_of_one_course_both_succeed() {
        let repo = repo().await;
        let store = Store::at(repo.path());
        let course = store.open(Some("raft"), at_new_year()).unwrap();

        std::thread::scope(|scope| {
            for _ in 0..2 {
                scope.spawn(|| {
                    for _ in 0..200 {
                        store.save(&course).unwrap();
                    }
                });
            }
        });

        assert_eq!(files(repo.path()), [".learn/raft/course.json", "a.txt"]);
        assert_eq!(store.open(Some("raft"), at_new_year()).unwrap(), course);
    }

    #[tokio::test]
    async fn a_corrupt_course_is_an_error_and_stays_on_disk() {
        let repo = repo().await;
        let store = Store::at(repo.path());
        store.open(Some("raft"), at_new_year()).unwrap();
        let path = repo.path().join(".learn/raft/course.json");
        fs::write(&path, "{ this is not json").unwrap();

        let error = store
            .open(Some("raft"), at_new_year())
            .unwrap_err()
            .to_string();
        assert!(error.contains("repair it by hand"), "{error}");
        assert_eq!(store.listing().unwrap(), "raft  (unreadable)\n");

        assert_eq!(fs::read_to_string(&path).unwrap(), "{ this is not json");
    }

    #[tokio::test]
    async fn a_slug_with_another_character_is_refused() {
        let repo = repo().await;
        let store = Store::at(repo.path());

        for slug in ["", "Raft", "raft_1", "../escape", "a/b", "."] {
            let error = store.open(Some(slug), at_new_year()).unwrap_err();
            assert!(error.to_string().contains("not a slug"), "{slug}: {error}");
        }
        store.open(Some("raft-2"), at_new_year()).unwrap();
        assert_eq!(files(repo.path()), [".learn/raft-2/course.json", "a.txt"]);
    }

    #[tokio::test]
    async fn a_slug_that_names_a_command_is_refused_and_stores_nothing() {
        let repo = repo().await;
        let store = Store::at(repo.path()).refusing(["list".to_owned(), "wait".to_owned()]);

        for slug in ["list", "wait"] {
            let error = store.open(Some(slug), at_new_year()).unwrap_err();
            assert_eq!(
                error.to_string(),
                format!(
                    "`{slug}` is a command of qlearn: `qlearn {slug}` runs it, so this course \
                     could never open. Pick another slug"
                )
            );
        }
        assert_eq!(files(repo.path()), ["a.txt"]);
        store.open(Some("listing"), at_new_year()).unwrap();
        assert_eq!(files(repo.path()), [".learn/listing/course.json", "a.txt"]);
    }

    /// A course made by an earlier release, or before its slug became a
    /// command, still opens by name: `qlearn -- list` reaches it.
    #[tokio::test]
    async fn a_course_whose_slug_became_a_command_still_opens() {
        let repo = repo().await;
        Store::at(repo.path())
            .open(Some("list"), at_new_year())
            .unwrap();
        Store::at(repo.path())
            .open(Some("raft"), at_new_year())
            .unwrap();
        let store = Store::at(repo.path()).refusing(["list".to_owned()]);

        assert_eq!(
            store.open(Some("list"), at_new_year()).unwrap().slug,
            "list"
        );
    }

    #[tokio::test]
    async fn without_a_slug_the_only_course_opens() {
        let repo = repo().await;
        let store = Store::at(repo.path());
        store.open(Some("raft"), at_new_year()).unwrap();
        // A file of the store is not a course.
        write(&repo.path().join(".learn/server.json"), "{}");

        assert_eq!(store.open(None, at_new_year()).unwrap().slug, "raft");
    }

    #[tokio::test]
    async fn without_a_slug_several_courses_are_listed() {
        let repo = repo().await;
        let store = Store::at(repo.path());
        store.open(Some("raft"), at_new_year()).unwrap();
        store.open(Some("paxos"), at_new_year()).unwrap();

        let error = store.open(None, at_new_year()).unwrap_err().to_string();
        assert!(error.contains("several courses: paxos, raft"), "{error}");
    }

    #[tokio::test]
    async fn without_a_slug_and_no_course_a_slug_is_asked_for() {
        let repo = repo().await;
        let store = Store::at(repo.path());

        let error = store.open(None, at_new_year()).unwrap_err().to_string();
        assert!(error.contains("qlearn <slug>"), "{error}");
        assert_eq!(files(repo.path()), ["a.txt"], "nothing is created");
    }

    #[tokio::test]
    async fn the_listing_gives_the_slug_the_status_and_the_title() {
        let repo = repo().await;
        let store = Store::at(repo.path());
        assert_eq!(store.listing().unwrap(), ".learn/ holds no course yet.\n");

        let mut raft = store.open(Some("raft"), at_new_year()).unwrap();
        raft.title = "Raft elections".to_owned();
        raft.status = CourseStatus::Running;
        store.save(&raft).unwrap();
        store.open(Some("paxos"), at_new_year()).unwrap();
        write(&repo.path().join(".learn/bully/notes.txt"), "bully\n");
        write(
            &repo.path().join(".learn/gossip/course.json"),
            r#"{"version":2}"#,
        );
        write(&repo.path().join(".learn/zab/course.json"), "{ not json");

        assert_eq!(
            store.listing().unwrap(),
            "gossip  (newer qlearn)\n\
             paxos  new  paxos\n\
             raft  running  Raft elections\n\
             zab  (unreadable)\n"
        );
    }

    fn text_page(html: &str) -> PageFile {
        PageFile {
            blocks: vec![Block::Text {
                id: "0123456789ab".to_owned(),
                html: html.to_owned(),
            }],
            hidden: BTreeMap::new(),
            quizzes: Vec::new(),
            threads: Vec::new(),
        }
    }

    #[tokio::test]
    async fn a_page_is_written_as_its_source_and_its_blocks() {
        let repo = repo().await;
        let store = Store::at(repo.path());
        store.open(Some("raft"), at_new_year()).unwrap();
        let page = text_page("<p>One.</p>");

        store.save_page("raft", "7a", "One.\n", &page).unwrap();

        assert_eq!(
            fs::read_to_string(repo.path().join(".learn/raft/pages/7a.md")).unwrap(),
            "One.\n"
        );
        assert_eq!(store.load_page("raft", "7a").unwrap(), Some(page));
        assert_eq!(store.load_page("raft", "8").unwrap(), None);
        assert_eq!(
            files(repo.path()),
            [
                ".learn/raft/course.json",
                ".learn/raft/pages/7a.json",
                ".learn/raft/pages/7a.md",
                "a.txt"
            ]
        );
    }

    /// The text of a hidden line is kept for the day the page may show it,
    /// and the blocks as shown never carry it.
    #[tokio::test]
    async fn a_hidden_line_is_stored_apart_from_the_blocks() {
        let repo = repo().await;
        let store = Store::at(repo.path());
        store.open(Some("raft"), at_new_year()).unwrap();
        let line = |text: &str| Line {
            text: text.to_owned(),
            spans: Vec::new(),
        };
        let page = PageFile {
            blocks: vec![Block::Excerpt {
                id: "0123456789ab".to_owned(),
                excerpt: model::Excerpt {
                    path: "a.c".to_owned(),
                    repo: String::new(),
                    commit: "0".repeat(40),
                    from: 1,
                    to: 2,
                    lang: "c".to_owned(),
                    lines: vec![Some(line("int a;")), None],
                    marked: Vec::new(),
                },
            }],
            hidden: BTreeMap::from([("0123456789ab".to_owned(), line("int b;"))]),
            quizzes: Vec::new(),
            threads: Vec::new(),
        };

        store.save_page("raft", "1", "source", &page).unwrap();
        let read = store.load_page("raft", "1").unwrap().unwrap();

        assert_eq!(read, page);
        assert_eq!(read.shown(), page.blocks);
        let shown = serde_json::to_string(&read.shown()).unwrap();
        assert!(!shown.contains("int b;"), "{shown}");

        // A quiz with no answer keeps the line hidden.
        let mut quizzed = read.clone();
        quizzed.quizzes.push(quiz());
        assert_eq!(quizzed.shown(), page.blocks);
        let sent = serde_json::to_string(&quizzed.page("1", &[])).unwrap();
        assert!(!sent.contains("int b;"), "{sent}");

        // The open answer alone leaves a part of the quiz to answer.
        quizzed.quizzes[0].sent.open = Some("A new term.".to_owned());
        assert_eq!(quizzed.shown(), page.blocks);

        // Once every quiz is answered, the blocks show the line in its place.
        quizzed.quizzes[0].sent.choices = Some(vec![crate::quiz::StoredPick {
            option: 0,
            other: None,
        }]);
        let Block::Excerpt { excerpt, .. } = &quizzed.shown()[0] else {
            panic!("an excerpt");
        };
        assert_eq!(excerpt.lines, [Some(line("int a;")), Some(line("int b;"))]);

        // A second quiz with no answer hides it again.
        quizzed.quizzes.push(quiz());
        assert_eq!(quizzed.shown(), page.blocks);
    }

    fn quiz() -> StoredQuiz {
        StoredQuiz {
            open: Some(crate::quiz::StoredOpen {
                prompt: "Why is the timeout random?".to_owned(),
                points: vec!["random-timeout".to_owned()],
            }),
            choices: vec![crate::quiz::StoredChoice {
                header: "Split vote".to_owned(),
                prompt: "What happens next?".to_owned(),
                options: vec!["A new term".to_owned(), "The higher id wins".to_owned()],
                answer: 0,
                point: "split-vote".to_owned(),
            }],
            sent: crate::quiz::Sent::default(),
            debrief: None,
        }
    }

    /// A quiz rewrites the blocks of its page and leaves the source alone:
    /// the source says which page the agent pushed last.
    #[tokio::test]
    async fn a_quiz_is_stored_with_its_answers_beside_the_blocks() {
        let repo = repo().await;
        let store = Store::at(repo.path());
        store.open(Some("raft"), at_new_year()).unwrap();
        let mut page = text_page("<p>One.</p>");
        store.save_page("raft", "1", "One.\n", &page).unwrap();
        let source = repo.path().join(".learn/raft/pages/1.md");
        let modified = fs::metadata(&source).unwrap().modified().unwrap();

        page.quizzes.push(quiz());
        let undo = store.save_blocks("raft", "1", &page).unwrap();

        assert_eq!(store.load_page("raft", "1").unwrap(), Some(page.clone()));
        let stored = fs::read_to_string(repo.path().join(".learn/raft/pages/1.json")).unwrap();
        assert!(stored.contains("\"answer\": 0"), "{stored}");
        assert_eq!(fs::read_to_string(&source).unwrap(), "One.\n");
        assert_eq!(fs::metadata(&source).unwrap().modified().unwrap(), modified);

        undo.undo().unwrap();
        assert_eq!(
            store.load_page("raft", "1").unwrap(),
            Some(text_page("<p>One.</p>"))
        );
    }

    #[tokio::test]
    async fn a_course_put_back_is_as_it_was() {
        let repo = repo().await;
        let store = Store::at(repo.path());
        let course = store.open(Some("raft"), at_new_year()).unwrap();
        let path = repo.path().join(".learn/raft/course.json");
        let before = fs::read(&path).unwrap();
        let mut changed = course.clone();
        changed.title = "Changed".to_owned();

        let undo = store.save_undoable(&changed).unwrap();
        assert_eq!(store.load("raft").unwrap(), Some(changed));
        undo.undo().unwrap();

        assert_eq!(fs::read(&path).unwrap(), before);
        assert_eq!(store.load("raft").unwrap(), Some(course));
    }

    #[tokio::test]
    async fn the_points_are_written_and_read_back() {
        let repo = repo().await;
        let store = Store::at(repo.path());
        store.open(Some("raft"), at_new_year()).unwrap();
        assert_eq!(store.load_points("raft").unwrap(), []);
        let point = Point {
            id: "split-vote".to_owned(),
            label: "How a split vote ends".to_owned(),
            lesson: "3".to_owned(),
            results: Vec::new(),
        };

        let first = store
            .save_points("raft", std::slice::from_ref(&point))
            .unwrap();
        let second = store.save_points("raft", &[]).unwrap();
        assert_eq!(store.load_points("raft").unwrap(), []);
        first.and(second).undo().unwrap();

        assert!(!repo.path().join(".learn/raft/points.json").exists());
        store
            .save_points("raft", std::slice::from_ref(&point))
            .unwrap();
        assert_eq!(store.load_points("raft").unwrap(), [point]);
        assert_eq!(
            files(repo.path()),
            [
                ".learn/raft/course.json",
                ".learn/raft/points.json",
                "a.txt"
            ]
        );
    }

    #[tokio::test]
    async fn corrupt_points_are_an_error_and_stay_on_disk() {
        let repo = repo().await;
        let store = Store::at(repo.path());
        let path = repo.path().join(".learn/raft/points.json");
        write(&path, "{ not json");

        let error = store.load_points("raft").unwrap_err().to_string();

        assert!(error.contains("repair it by hand"), "{error}");
        assert_eq!(fs::read_to_string(&path).unwrap(), "{ not json");
    }

    #[tokio::test]
    async fn the_choices_are_written_and_read_back() {
        let repo = repo().await;
        let store = Store::at(repo.path());
        store.open(Some("raft"), at_new_year()).unwrap();
        assert_eq!(store.load_choices("raft").unwrap(), []);
        let choice = StoredChoosing {
            id: "choice-1".to_owned(),
            page: None,
            questions: Vec::new(),
            at: "2026-01-01T00:00:00Z".to_owned(),
            answered: None,
        };

        store
            .save_choices("raft", std::slice::from_ref(&choice))
            .unwrap();

        assert_eq!(store.load_choices("raft").unwrap(), [choice]);
        assert_eq!(
            files(repo.path()),
            [
                ".learn/raft/choices.json",
                ".learn/raft/course.json",
                "a.txt"
            ]
        );
    }

    #[tokio::test]
    async fn corrupt_choices_are_an_error_and_stay_on_disk() {
        let repo = repo().await;
        let store = Store::at(repo.path());
        let path = repo.path().join(".learn/raft/choices.json");
        write(&path, "{ not json");

        let error = store.load_choices("raft").unwrap_err().to_string();

        assert!(error.contains("repair it by hand"), "{error}");
        assert_eq!(fs::read_to_string(&path).unwrap(), "{ not json");
    }

    #[tokio::test]
    async fn a_page_id_that_could_leave_the_store_is_refused() {
        let repo = repo().await;
        let store = Store::at(repo.path());
        store.open(Some("raft"), at_new_year()).unwrap();

        for id in ["../course", "", "1/2", "a"] {
            let error = store.save_page("raft", id, "x", &PageFile::default());
            assert!(error.is_err(), "{id:?}");
            assert!(store.load_page("raft", id).is_err(), "{id:?}");
        }
        assert_eq!(files(repo.path()), [".learn/raft/course.json", "a.txt"]);
    }

    #[tokio::test]
    async fn a_corrupt_page_is_an_error_and_stays_on_disk() {
        let repo = repo().await;
        let store = Store::at(repo.path());
        let path = repo.path().join(".learn/raft/pages/1.json");
        write(&path, "{ not json");

        let error = store.load_page("raft", "1").unwrap_err().to_string();

        assert!(error.contains("repair it by hand"), "{error}");
        assert!(!error.contains("push the page again"), "{error}");
        assert_eq!(fs::read_to_string(&path).unwrap(), "{ not json");
    }

    /// A page of a later release carries more than blocks, and a page of
    /// an earlier one has no quizzes and no threads. A missing field reads
    /// as empty, and an unknown one is left aside.
    #[tokio::test]
    async fn a_page_file_reads_with_a_field_more_or_a_field_less() {
        let repo = repo().await;
        let store = Store::at(repo.path());
        let dir = repo.path().join(".learn/raft/pages");
        write(&dir.join("1.json"), r#"{ "blocks": [], "charts": [{}] }"#);
        write(&dir.join("2.json"), "{}");

        assert_eq!(
            store.load_page("raft", "1").unwrap(),
            Some(PageFile::default())
        );
        assert_eq!(
            store.load_page("raft", "2").unwrap(),
            Some(PageFile::default())
        );
    }

    fn taught(course: &mut Course, ids: &[&str]) {
        for id in ids {
            let kind = PageKind::of_id(id).unwrap();
            course.push_page(id, kind, None, at_new_year());
        }
    }

    fn set_modified(path: &Path, at: SystemTime) {
        fs::File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_modified(at)
            .unwrap();
    }

    #[tokio::test]
    async fn the_current_page_is_the_one_pushed_last() {
        let repo = repo().await;
        let store = Store::at(repo.path());
        let mut course = store.open(Some("raft"), at_new_year()).unwrap();
        assert_eq!(store.current(&course).unwrap(), None);

        taught(&mut course, &["1", "2"]);
        let page = text_page("<p>One.</p>");
        store.save_page("raft", "1", "One.\n", &page).unwrap();
        store.save_page("raft", "2", "Two.\n", &page).unwrap();
        // The clock of the file system may not move between two writes.
        let now = SystemTime::now();
        let pages = repo.path().join(".learn/raft/pages");
        set_modified(
            &pages.join("1.md"),
            now - std::time::Duration::from_secs(120),
        );
        set_modified(
            &pages.join("2.md"),
            now - std::time::Duration::from_secs(60),
        );
        // An answer of the learner rewrites the blocks of page 1 later.
        set_modified(&pages.join("1.json"), now);
        // Neither a stray file nor a temporary one is a page.
        write(&pages.join("notes.md"), "Notes.\n");

        assert_eq!(store.current(&course).unwrap().as_deref(), Some("2"));
    }

    /// A push that stopped before `course.json` leaves page files that the
    /// syllabus does not back. They name no page.
    #[tokio::test]
    async fn a_page_file_that_the_course_does_not_hold_is_not_current() {
        let repo = repo().await;
        let store = Store::at(repo.path());
        let mut course = store.open(Some("raft"), at_new_year()).unwrap();
        taught(&mut course, &["1"]);
        let page = text_page("<p>One.</p>");
        store.save_page("raft", "1", "One.\n", &page).unwrap();
        store.save_page("raft", "2", "Two.\n", &page).unwrap();
        let earlier = SystemTime::now() - std::time::Duration::from_secs(60);
        set_modified(&repo.path().join(".learn/raft/pages/1.md"), earlier);

        assert_eq!(store.current(&course).unwrap().as_deref(), Some("1"));
    }

    #[tokio::test]
    async fn a_page_put_back_leaves_the_files_as_they_were() {
        let repo = repo().await;
        let store = Store::at(repo.path());
        store.open(Some("raft"), at_new_year()).unwrap();
        let one = text_page("<p>One.</p>");
        store.save_page("raft", "1", "One.\n", &one).unwrap();

        let undo = store
            .save_page("raft", "1", "Changed.\n", &text_page("<p>Changed.</p>"))
            .unwrap();
        undo.undo().unwrap();
        let undo = store.save_page("raft", "2", "Two.\n", &one).unwrap();
        undo.undo().unwrap();

        assert_eq!(store.load_page("raft", "1").unwrap(), Some(one));
        assert_eq!(
            fs::read_to_string(repo.path().join(".learn/raft/pages/1.md")).unwrap(),
            "One.\n"
        );
        assert_eq!(
            files(repo.path()),
            [
                ".learn/raft/course.json",
                ".learn/raft/pages/1.json",
                ".learn/raft/pages/1.md",
                "a.txt"
            ]
        );
    }

    #[tokio::test]
    async fn the_cheatsheet_is_written_read_back_and_put_back() {
        let repo = repo().await;
        let store = Store::at(repo.path());
        store.open(Some("raft"), at_new_year()).unwrap();
        let sheet = CheatsheetFile {
            blocks: text_page("<p>One.</p>").blocks,
        };
        assert_eq!(store.load_cheatsheet("raft").unwrap(), None);

        store.save_cheatsheet("raft", "One.\n", &sheet).unwrap();
        let undo = store
            .save_cheatsheet("raft", "Two.\n", &CheatsheetFile::default())
            .unwrap();
        undo.undo().unwrap();

        assert_eq!(store.load_cheatsheet("raft").unwrap(), Some(sheet));
        assert_eq!(
            fs::read_to_string(repo.path().join(".learn/raft/cheatsheet.md")).unwrap(),
            "One.\n"
        );
        assert_eq!(
            files(repo.path()),
            [
                ".learn/raft/cheatsheet.json",
                ".learn/raft/cheatsheet.md",
                ".learn/raft/course.json",
                "a.txt"
            ]
        );
    }

    #[tokio::test]
    async fn a_corrupt_cheatsheet_is_an_error_and_stays_on_disk() {
        let repo = repo().await;
        let store = Store::at(repo.path());
        let path = repo.path().join(".learn/raft/cheatsheet.json");
        write(&path, "{ not json");

        let error = store.load_cheatsheet("raft").unwrap_err().to_string();

        assert!(error.contains("qlearn cheatsheet"), "{error}");
        assert_eq!(fs::read_to_string(&path).unwrap(), "{ not json");
    }

    #[tokio::test]
    async fn a_link_in_place_of_the_pages_is_refused_and_left_alone() {
        let repo = repo().await;
        let store = Store::at(repo.path());
        let mut course = store.open(Some("raft"), at_new_year()).unwrap();
        taught(&mut course, &["1"]);
        write(&repo.path().join("src/1.md"), "The code.\n");
        std::os::unix::fs::symlink("../../src", repo.path().join(".learn/raft/pages")).unwrap();

        let page = text_page("<p>One.</p>");
        let error = store
            .save_page("raft", "1", "One.\n", &page)
            .unwrap_err()
            .to_string();
        assert!(error.contains("symbolic link"), "{error}");
        assert!(store.load_page("raft", "1").is_err());
        assert!(store.current(&course).is_err());

        assert_eq!(
            fs::read_to_string(repo.path().join("src/1.md")).unwrap(),
            "The code.\n"
        );
        assert_eq!(files(&repo.path().join("src")), ["1.md"]);
    }

    fn media_name(bytes: &[u8], ext: &str) -> String {
        use sha2::{Digest, Sha256};
        format!("{}.{ext}", hex::encode(Sha256::digest(bytes)))
    }

    /// A name is the hash of its bytes, so a name already there is never
    /// written again, and the undo of a failed push removes only the
    /// images that it added.
    #[tokio::test]
    async fn an_image_is_written_once_under_the_hash_of_its_bytes() {
        let repo = repo().await;
        let store = Store::at(repo.path());
        store.open(Some("raft"), at_new_year()).unwrap();
        let (one, two) = (media_name(b"one", "png"), media_name(b"two", "svg"));
        let first = BTreeMap::from([(one.clone(), b"one".to_vec())]);
        store.save_media("raft", &first).unwrap();
        let dir = repo.path().join(".learn/raft/media");
        let written = fs::metadata(dir.join(&one)).unwrap().modified().unwrap();

        let both = BTreeMap::from([
            (one.clone(), b"one".to_vec()),
            (two.clone(), b"two".to_vec()),
        ]);
        let undo = store.save_media("raft", &both).unwrap();

        let mut both_names = vec![one.clone(), two.clone()];
        both_names.sort();
        assert_eq!(files(&dir), both_names);
        assert_eq!(
            fs::metadata(dir.join(&one)).unwrap().modified().unwrap(),
            written,
            "no second write"
        );
        assert_eq!(store.load_media("raft", &two).unwrap().unwrap(), b"two");
        undo.undo().unwrap();
        assert_eq!(files(&dir), [one]);
        assert_eq!(store.load_media("raft", &two).unwrap(), None);
    }

    #[tokio::test]
    async fn a_name_that_is_not_a_hash_reads_and_writes_nothing() {
        let repo = repo().await;
        let store = Store::at(repo.path());
        store.open(Some("raft"), at_new_year()).unwrap();

        for name in ["../course.json", "course.json", "", "a.png"] {
            assert_eq!(store.load_media("raft", name).unwrap(), None, "{name}");
            let media = BTreeMap::from([(name.to_owned(), b"x".to_vec())]);
            assert!(store.save_media("raft", &media).is_err(), "{name}");
        }
        assert_eq!(files(&repo.path().join(".learn")), ["raft/course.json"]);
    }

    #[tokio::test]
    async fn a_link_in_place_of_the_media_is_refused_and_left_alone() {
        let repo = repo().await;
        let store = Store::at(repo.path());
        store.open(Some("raft"), at_new_year()).unwrap();
        write(&repo.path().join("src/a.txt"), "The code.\n");
        std::os::unix::fs::symlink("../../src", repo.path().join(".learn/raft/media")).unwrap();
        let name = media_name(b"one", "png");

        let media = BTreeMap::from([(name.clone(), b"one".to_vec())]);
        let error = store.save_media("raft", &media).unwrap_err().to_string();

        assert!(error.contains("symbolic link"), "{error}");
        assert!(store.load_media("raft", &name).is_err());
        assert_eq!(files(&repo.path().join("src")), ["a.txt"]);
    }

    #[tokio::test]
    async fn the_only_course_is_read_and_never_made() {
        let repo = repo().await;
        let store = Store::at(repo.path());
        let error = store.only().unwrap_err().to_string();
        assert!(error.contains("holds no course yet"), "{error}");
        assert_eq!(files(repo.path()), ["a.txt"], "nothing is created");

        let course = store.open(Some("raft"), at_new_year()).unwrap();
        assert_eq!(store.only().unwrap(), course);
        store.open(Some("paxos"), at_new_year()).unwrap();
        let error = store.only().unwrap_err().to_string();
        assert!(error.contains("several courses: paxos, raft"), "{error}");
    }

    /// The names of the notes, never their content, and never what a link
    /// leads to.
    #[tokio::test]
    async fn the_notes_are_listed_by_name_and_a_link_is_not_followed() {
        let repo = repo().await;
        let store = Store::at(repo.path());
        store.open(Some("raft"), at_new_year()).unwrap();
        assert_eq!(store.notes("raft").unwrap(), Vec::<String>::new());
        let notes = repo.path().join(".learn/raft/notes");
        write(&notes.join("plan.md"), "The plan.\n");
        write(&notes.join("sources/paper.txt"), "The paper.\n");
        let elsewhere = tempfile::tempdir().unwrap();
        write(&elsewhere.path().join("secret.txt"), "secret\n");
        std::os::unix::fs::symlink(elsewhere.path(), notes.join("away")).unwrap();

        assert_eq!(
            store.notes("raft").unwrap(),
            ["away", "plan.md", "sources/paper.txt"]
        );
    }

    #[tokio::test]
    async fn a_removal_names_what_the_course_holds_and_removes_it_alone() {
        let repo = repo().await;
        let store = Store::at(repo.path());
        let mut course = store.open(Some("raft"), at_new_year()).unwrap();
        course.title = "Raft elections".to_owned();
        store.save(&course).unwrap();
        store.open(Some("paxos"), at_new_year()).unwrap();
        let page = text_page("<p>One.</p>");
        store.save_page("raft", "1", "One.\n", &page).unwrap();
        store.save_page("raft", "c1", "Check.\n", &page).unwrap();
        let dir = repo.path().join(".learn/raft");
        // A source with no blocks is a write that failed: no cheatsheet.
        write(&dir.join("cheatsheet.md"), "# Raft\n");
        assert!(!store.removal("raft").unwrap().cheatsheet);
        write(&dir.join("cheatsheet.json"), "{}\n");
        write(&dir.join("notes/plan.md"), "The plan.\n");
        // A link inside the course leads out of the store, and stays a link.
        let elsewhere = tempfile::tempdir().unwrap();
        write(&elsewhere.path().join("kept.txt"), "kept\n");
        std::os::unix::fs::symlink(elsewhere.path(), dir.join("notes/away")).unwrap();

        let removal = store.removal("raft").unwrap();
        assert_eq!(
            removal,
            Removal {
                slug: "raft".to_owned(),
                dir: dir.clone(),
                title: Some("Raft elections".to_owned()),
                pages: 2,
                cheatsheet: true,
                notes: 2,
            }
        );
        store.remove(&removal).unwrap();

        assert!(!dir.exists());
        assert_eq!(files(elsewhere.path()), ["kept.txt"]);
        assert_eq!(files(repo.path()), [".learn/paxos/course.json", "a.txt"]);
    }

    /// A `course.json` that does not parse, that a newer qlearn wrote, or
    /// that the system cannot read, holds the course back from no delete.
    #[tokio::test]
    async fn an_unreadable_course_may_be_removed() {
        let repo = repo().await;
        let store = Store::at(repo.path());
        let path = repo.path().join(".learn/raft/course.json");
        let newer = format!("{{ \"version\": {} }}", model::VERSION + 1);
        for text in ["{ not json", newer.as_str()] {
            write(&path, text);

            let removal = store.removal("raft").unwrap();

            assert_eq!(removal.title, None, "{text}");
            assert_eq!(
                (removal.pages, removal.cheatsheet, removal.notes),
                (0, false, 0)
            );
        }

        // A read fails for a directory as it does for a file with no
        // permission, and the test may run as root.
        fs::remove_file(&path).unwrap();
        fs::create_dir(&path).unwrap();
        let removal = store.removal("raft").unwrap();
        assert_eq!(removal.title, None);
        store.remove(&removal).unwrap();
        assert!(!repo.path().join(".learn/raft").exists());
    }

    #[tokio::test]
    async fn a_removal_of_what_is_no_course_is_refused_and_removes_nothing() {
        let repo = repo().await;
        let store = Store::at(repo.path());
        let error = store.removal("raft").unwrap_err().to_string();
        assert_eq!(error, ".learn/ holds no course yet, so no course raft");

        store.open(Some("paxos"), at_new_year()).unwrap();
        store.open(Some("zab"), at_new_year()).unwrap();
        let error = store.removal("raft").unwrap_err().to_string();
        assert_eq!(
            error,
            ".learn/ holds no course raft. Its courses: paxos, zab"
        );

        write(&repo.path().join(".learn/bully/notes.txt"), "bully\n");
        let error = store.removal("bully").unwrap_err().to_string();
        assert!(error.contains("not a course of qlearn"), "{error}");

        let elsewhere = tempfile::tempdir().unwrap();
        write(&elsewhere.path().join("course.json"), "{}");
        std::os::unix::fs::symlink(elsewhere.path(), repo.path().join(".learn/gossip")).unwrap();
        let error = store.removal("gossip").unwrap_err().to_string();
        assert!(error.contains("symbolic link"), "{error}");

        for slug in ["../paxos", "", "Raft"] {
            let error = store.removal(slug).unwrap_err().to_string();
            assert!(error.contains("not a slug"), "{slug}: {error}");
        }
        assert_eq!(files(elsewhere.path()), ["course.json"]);
        assert_eq!(
            files(repo.path()),
            [
                ".learn/bully/notes.txt",
                // Through the link, which stays.
                ".learn/gossip/course.json",
                ".learn/paxos/course.json",
                ".learn/zab/course.json",
                "a.txt"
            ]
        );
    }

    fn exclude_file(repo: &Repo) -> String {
        fs::read_to_string(repo.path().join(".git/info/exclude")).unwrap_or_default()
    }

    fn count(text: &str) -> usize {
        text.lines().filter(|line| *line == ".learn/").count()
    }

    #[tokio::test]
    async fn the_store_is_excluded_once() {
        let repo = repo().await;
        let git = Git::discover(repo.path()).await.unwrap();
        // A last line of the user without its newline.
        let before = format!("{}*.log", exclude_file(&repo));
        fs::write(repo.path().join(".git/info/exclude"), &before).unwrap();

        exclude(&git).await.unwrap();
        exclude(&git).await.unwrap();

        assert_eq!(exclude_file(&repo), format!("{before}\n.learn/\n"));

        Store::at(repo.path())
            .open(Some("raft"), at_new_year())
            .unwrap();
        assert_eq!(repo.git(&["status", "--porcelain"]).await, "");
    }

    #[tokio::test]
    async fn a_store_that_git_ignores_already_is_left_alone() {
        let repo = build_repo(&[commit("first").file(".gitignore", ".learn/\n")]).await;
        let git = Git::discover(repo.path()).await.unwrap();
        let before = exclude_file(&repo);

        exclude(&git).await.unwrap();

        assert_eq!(exclude_file(&repo), before);
    }

    #[tokio::test]
    async fn a_store_that_git_cannot_ignore_is_excluded_once() {
        // A .gitignore that un-ignores the store wins over info/exclude, and
        // git never calls a tracked path ignored.
        for spec in [
            commit("first").file(".gitignore", "!.learn/\n"),
            commit("first").file(".learn/raft/course.json", "{}\n"),
        ] {
            let repo = build_repo(&[spec]).await;
            let git = Git::discover(repo.path()).await.unwrap();

            exclude(&git).await.unwrap();
            exclude(&git).await.unwrap();

            assert_eq!(count(&exclude_file(&repo)), 1);
        }
    }

    #[tokio::test]
    async fn a_linked_worktree_is_excluded_through_the_common_directory() {
        let repo = repo().await;
        let elsewhere = tempfile::tempdir().unwrap();
        let tree = elsewhere.path().join("second-worktree");
        repo.git(&["worktree", "add", tree.to_str().unwrap(), "-b", "other"])
            .await;

        let git = Git::discover(&tree).await.unwrap();
        exclude(&git).await.unwrap();

        assert_eq!(count(&exclude_file(&repo)), 1);
        assert_eq!(
            files(&tree),
            ["a.txt"],
            "nothing is written in the worktree"
        );
    }

    #[tokio::test]
    async fn every_write_stays_under_the_store() {
        let repo = repo().await;
        let git = Git::discover(repo.path()).await.unwrap();
        let config = fs::read_to_string(repo.path().join(".git/config")).unwrap();

        exclude(&git).await.unwrap();
        let store = Store::at(repo.path());
        store.open(Some("raft"), at_new_year()).unwrap();
        store.open(Some("paxos"), at_new_year()).unwrap();

        let outside: Vec<_> = files(repo.path())
            .into_iter()
            .filter(|f| !f.starts_with(".learn/"))
            .collect();
        assert_eq!(outside, ["a.txt"]);
        assert_eq!(
            fs::read_to_string(repo.path().join(".git/config")).unwrap(),
            config
        );
    }
}
