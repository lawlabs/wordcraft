//! A penalty-calculation block: prose and a table whose values are the same content controls.
//!
//! The right-hand pane edits those values and writes them back into every marked place. Nothing
//! is calculated yet: the penalty amount is stored text, the same as the debt and the dates.

use serde_json::{Value, json};
use wordcraft_doc::control::{ContentControl, ControlKind, ControlRange};
use wordcraft_doc::para::OBJ;
use wordcraft_doc::props::CharProps;
use wordcraft_doc::{Block, Cell, Document, Paragraph, Path, Pos, StoryRef, Table};

use super::sel_result;
use crate::{CmdError, CmdResult, CommandSpec, Selection, Session, p};

/// Tag prefix of a value that belongs to a penalty calculation (`lawmatic.penalty.<group>.<key>`).
pub const TAG_PREFIX: &str = "lawmatic.penalty.";

const LOC: &str = "Insert › Calculations";
const MAX_VALUE: usize = 200;

struct Slot {
    key: &'static str,
    /// English label for the pane (the catalog translates it).
    label: &'static str,
    /// Title stored on the control, shown when the caret is in that value.
    title: &'static str,
    sample: &'static str,
    /// Written into the sentence.
    prose: bool,
    /// A row of the table.
    table: bool,
}

const SLOTS: &[Slot] = &[
    Slot { key: "title", label: "Calculation title", title: "Заголовок", sample: "Расчёт пеней", prose: false, table: false },
    Slot { key: "debt", label: "Debt amount", title: "Сумма долга", sample: "100 000 руб.", prose: true, table: true },
    Slot { key: "rate", label: "Penalty rate", title: "Ставка", sample: "1/300 ключевой ставки", prose: true, table: true },
    Slot { key: "from", label: "Period start", title: "Дата начала", sample: "01.01.2026", prose: true, table: true },
    Slot { key: "to", label: "Period end", title: "Дата окончания", sample: "01.03.2026", prose: true, table: true },
    Slot { key: "days", label: "Days overdue", title: "Дней просрочки", sample: "59", prose: true, table: true },
    Slot { key: "penalty", label: "Penalty amount", title: "Сумма пеней", sample: "19 633,33 руб.", prose: true, table: true },
];

/// One editable value of the calculation the caret is in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PenaltyField {
    pub key: &'static str,
    pub label: &'static str,
    pub value: String,
}

/// The calculation block around the caret.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PenaltyBlock {
    pub group: String,
    pub fields: Vec<PenaltyField>,
}

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("penalty.insert", "Penalty calculation", LOC, insert),
        CommandSpec::new("penalty.set", "Set Penalty Value", LOC, set).params(r#"{"group": string, "key": string, "value": string}"#),
        CommandSpec::new("penalty.current", "Penalty Calculation at Caret", LOC, current).pure(),
    ]
}

/// The calculation whose marked text or table contains the caret, if any.
pub fn block_at(s: &Session) -> Option<PenaltyBlock> {
    let caret = &s.sel.focus;
    let ranges = s.doc.control_ranges(caret.story);
    let mut groups: Vec<(String, Path, Path)> = Vec::new();
    for r in &ranges {
        let Some((group, _)) = parse_tag(&r.control.tag) else { continue };
        if let Some((_, min, max)) = groups.iter_mut().find(|(g, _, _)| g == group) {
            if r.start.path < *min {
                *min = r.start.path.clone();
            }
            if r.end.path > *max {
                *max = r.end.path.clone();
            }
        } else {
            groups.push((group.to_string(), r.start.path.clone(), r.end.path.clone()));
        }
    }
    // Several calculations can sit in one story. Keep the tightest span that holds the caret.
    let mut hit: Option<(String, Path, Path)> = None;
    for (group, min, max) in groups {
        if caret.path < min || caret.path > max {
            continue;
        }
        let tighter = match &hit {
            None => true,
            Some((_, a, b)) => min >= *a && max <= *b && (&min, &max) != (a, b),
        };
        if tighter {
            hit = Some((group, min, max));
        }
    }
    let (group, _, _) = hit?;
    let fields = SLOTS
        .iter()
        .filter_map(|slot| {
            let value = field_value(&s.doc, &ranges, &group, slot.key)?;
            Some(PenaltyField { key: slot.key, label: slot.label, value })
        })
        .collect();
    Some(PenaltyBlock { group, fields })
}

fn current(s: &mut Session, _: &Value) -> CmdResult {
    Ok(match block_at(s) {
        Some(b) => json!({
            "group": b.group,
            "fields": b.fields.iter().map(|f| json!({"key": f.key, "label": f.label, "value": f.value})).collect::<Vec<_>>(),
        }),
        None => Value::Null,
    })
}

fn insert(s: &mut Session, _: &Value) -> CmdResult {
    let group = next_group(s);
    let ids = alloc_ids(&s.doc, control_count());
    let mut next = 0usize;
    let mut take_id = || {
        let id = ids.get(next).copied().unwrap_or(1_000_000);
        next = next.saturating_add(1);
        id
    };

    let heading = slot_para(&group, &mut take_id, "title")?.styled("Heading2");

    let mut prose = Paragraph::new();
    push_text(&mut prose, "Задолженность составляет ")?;
    push_slot(&mut prose, &group, &mut take_id, "debt")?;
    push_text(&mut prose, ". Ставка: ")?;
    push_slot(&mut prose, &group, &mut take_id, "rate")?;
    push_text(&mut prose, ". Период просрочки: с ")?;
    push_slot(&mut prose, &group, &mut take_id, "from")?;
    push_text(&mut prose, " по ")?;
    push_slot(&mut prose, &group, &mut take_id, "to")?;
    push_text(&mut prose, ", ")?;
    push_slot(&mut prose, &group, &mut take_id, "days")?;
    push_text(&mut prose, " дн. Сумма пеней: ")?;
    push_slot(&mut prose, &group, &mut take_id, "penalty")?;
    push_text(&mut prose, ".")?;

    let note = Paragraph::with_text(
        "Отмеченные значения связаны: панель справа меняет их и в этом тексте, и в таблице. Формула пока не считается.",
        CharProps::default(),
    );

    let width = s.doc.sections().first().map(|(_, sp)| sp.text_width()).unwrap_or(468.0);
    let table_slots: Vec<&Slot> = SLOTS.iter().filter(|slot| slot.table).collect();
    let mut table = Table::new(table_slots.len().saturating_add(1), 2, width);
    put_cell(&mut table, 0, 0, Paragraph::with_text("Показатель", bold()));
    put_cell(&mut table, 0, 1, Paragraph::with_text("Значение", bold()));
    for (i, slot) in table_slots.iter().enumerate() {
        put_cell(&mut table, i.saturating_add(1), 0, Paragraph::with_text(slot.title, CharProps::default()));
        let mut value = Paragraph::new();
        push_slot(&mut value, &group, &mut take_id, slot.key)?;
        put_cell(&mut table, i.saturating_add(1), 1, value);
    }

    let story = s.sel.focus.story;
    let at = super::delete_selection(s)?;
    let len = s.doc.para_at(&at).map(|para| para.len()).unwrap_or(0);
    let mut path = if at.off == 0 {
        at.path.clone()
    } else {
        let split = s.doc.split_paragraph(&at)?;
        if at.off >= len { at.path.with_last(at.path.last().saturating_add(1)) } else { split.path }
    };
    let first = path.clone();
    for block in [Block::Para(heading), Block::Para(prose), Block::Para(note), Block::Table(table)] {
        s.doc.insert_block(story, &path, block)?;
        path = path.with_last(path.last().saturating_add(1));
    }
    s.sel = Selection::caret(Pos::new(story, first, 0));
    s.goal_x = None;
    s.status = "Inserted a penalty calculation. The pane on the right edits its values.".into();
    sel_result(s)
}

fn set(s: &mut Session, v: &Value) -> CmdResult {
    let group = p::req_str(v, "group")?;
    let key = p::req_str(v, "key")?;
    let value = p::str(v, "value").unwrap_or("");
    if parse_tag(&format!("{TAG_PREFIX}{group}.{key}")).is_none() || SLOTS.iter().all(|slot| slot.key != key) {
        return Err(CmdError::Params("group and key must name a penalty value".into()));
    }
    let text = clean_value(value);
    let saved = s.sel.clone();
    let mut slots: Vec<ControlRange> =
        all_ranges(&s.doc).into_iter().filter(|r| parse_tag(&r.control.tag).is_some_and(|(g, k)| g == group && k == key)).collect();
    if slots.is_empty() {
        return Err(CmdError::Failed("no penalty value with that name".into()));
    }
    // Later places first, so an earlier control's position stays valid.
    slots.sort_by(|a, b| (&b.start.path, b.start.off).cmp(&(&a.start.path, a.start.off)));
    let placeholder = SLOTS.iter().find(|slot| slot.key == key).map(|slot| slot.title).unwrap_or("");
    for r in &slots {
        let start = r.content_start();
        let props = s.doc.para_at(&start).map(|para| para.props_at(start.off).clone()).unwrap_or_default();
        if start != r.end {
            s.doc.delete_range(&start, &r.end)?;
        }
        let shown = if text.is_empty() { placeholder } else { text.as_str() };
        if !shown.is_empty() {
            s.doc.insert_text(&start, shown, &props)?;
        }
        if let Some(c) = s.doc.control_mut(&r.start) {
            c.showing_placeholder = text.is_empty();
        }
    }
    s.sel = saved;
    clamp_sel(s);
    Ok(json!({"group": group, "key": key, "value": text}))
}

fn control_count() -> usize {
    SLOTS.iter().map(|slot| usize::from(slot.key == "title") + usize::from(slot.prose) + usize::from(slot.table)).sum()
}

fn slot_para(group: &str, take_id: &mut impl FnMut() -> i64, key: &str) -> Result<Paragraph, CmdError> {
    let mut para = Paragraph::new();
    push_slot(&mut para, group, take_id, key)?;
    Ok(para)
}

fn push_text(p: &mut Paragraph, text: &str) -> Result<(), CmdError> {
    if !text.is_empty() {
        p.insert_text(p.len(), text, &CharProps::default())?;
    }
    Ok(())
}

fn push_slot(p: &mut Paragraph, group: &str, take_id: &mut impl FnMut() -> i64, key: &str) -> Result<(), CmdError> {
    let Some(slot) = SLOTS.iter().find(|slot| slot.key == key) else {
        return Err(CmdError::Failed("unknown penalty value".into()));
    };
    let mut c = ContentControl::new(ControlKind::Text { multi_line: false });
    c.id = Some(take_id());
    c.tag = format!("{TAG_PREFIX}{group}.{key}");
    c.title = slot.title.to_string();
    c.placeholder_text = slot.title.to_string();
    let props = CharProps::default();
    p.insert_object(p.len(), wordcraft_doc::InlineObject::ControlStart { control: Box::new(c) }, &props)?;
    p.insert_text(p.len(), slot.sample, &props)?;
    p.insert_object(p.len(), wordcraft_doc::InlineObject::ControlEnd, &props)?;
    Ok(())
}

fn put_cell(table: &mut Table, row: usize, col: usize, para: Paragraph) {
    let Some(cell) = table.rows.get_mut(row).and_then(|r| r.cells.get_mut(col)) else { return };
    let props = cell.props.clone();
    *cell = Cell { props, blocks: vec![wordcraft_doc::para_block(para)], controls: Default::default() };
}

fn bold() -> CharProps {
    CharProps { bold: Some(true), ..CharProps::default() }
}

fn parse_tag(tag: &str) -> Option<(&str, &str)> {
    let rest = tag.strip_prefix(TAG_PREFIX)?;
    let (group, key) = rest.split_once('.')?;
    let ok = |s: &str| !s.is_empty() && s.len() <= 32 && s.bytes().all(|b| b.is_ascii_alphanumeric());
    (ok(group) && ok(key)).then_some((group, key))
}

fn field_value(doc: &Document, ranges: &[ControlRange], group: &str, key: &str) -> Option<String> {
    let r = ranges.iter().find(|r| parse_tag(&r.control.tag).is_some_and(|(g, k)| g == group && k == key))?;
    if r.control.showing_placeholder { Some(String::new()) } else { Some(doc.control_text(r)) }
}

fn clean_value(v: &str) -> String {
    v.chars().filter(|c| !c.is_control() && *c != OBJ).take(MAX_VALUE).collect::<String>().trim().to_string()
}

fn all_ranges(doc: &Document) -> Vec<ControlRange> {
    let mut out = doc.control_ranges(StoryRef::Body);
    for id in doc.parts.keys() {
        out.extend(doc.control_ranges(StoryRef::Part(*id)));
    }
    out
}

fn alloc_ids(doc: &Document, n: usize) -> Vec<i64> {
    let mut max = 1_000_000i64;
    for r in all_ranges(doc) {
        if let Some(id) = r.control.id {
            max = max.max(id);
        }
    }
    (1..=n).map(|i| max.saturating_add(i as i64)).collect()
}

fn next_group(s: &Session) -> String {
    let mut used = std::collections::BTreeSet::new();
    for r in all_ranges(&s.doc) {
        if let Some((group, _)) = parse_tag(&r.control.tag) {
            used.insert(group.to_string());
        }
    }
    let mut n = 1u32;
    while used.contains(&format!("p{n}")) && n < u32::MAX {
        n = n.saturating_add(1);
    }
    format!("p{n}")
}

fn clamp_sel(s: &mut Session) {
    for pos in [&mut s.sel.anchor, &mut s.sel.focus] {
        if let Some(para) = s.doc.para_at(pos) {
            pos.off = para.clamp(pos.off);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Session;

    #[test]
    fn values_in_the_sentence_and_the_table_stay_together() {
        let mut s = Session::new(Document::new());
        s.run("penalty.insert", &json!({})).unwrap();
        let block = block_at(&s).expect("caret is in the calculation");
        assert_eq!(block.fields.iter().find(|f| f.key == "debt").map(|f| f.value.as_str()), Some("100 000 руб."));
        let debt = |s: &Session| -> Vec<String> {
            s.doc.control_ranges(StoryRef::Body).iter().filter(|r| r.control.tag.ends_with(".debt")).map(|r| s.doc.control_text(r)).collect()
        };
        assert_eq!(debt(&s).len(), 2, "the sentence and the table each carry the debt");
        s.run("penalty.set", &json!({"group": block.group, "key": "debt", "value": "5 руб."})).unwrap();
        assert_eq!(debt(&s), vec!["5 руб.".to_string(), "5 руб.".to_string()]);
        let penalty: Vec<String> =
            s.doc.control_ranges(StoryRef::Body).iter().filter(|r| r.control.tag.ends_with(".penalty")).map(|r| s.doc.control_text(r)).collect();
        assert_eq!(penalty, vec!["19 633,33 руб.".to_string(), "19 633,33 руб.".to_string()], "no formula ran");
        assert!(block_at(&s).is_some(), "the caret stays inside the calculation");
        let outside = s.doc.para_paths(StoryRef::Body).into_iter().last().unwrap();
        s.sel = Selection::caret(Pos::new(StoryRef::Body, outside, 0));
        assert!(block_at(&s).is_none(), "a paragraph after the block is not the calculation");
    }
}
