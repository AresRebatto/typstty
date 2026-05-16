use typst_syntax::{parse, highlight, LinkedNode, Tag};

pub struct HighlightSpan {
    pub byte_range: std::ops::Range<usize>,
    pub color: egui::Color32,
}

pub fn compute_highlight(source: &str) -> Vec<HighlightSpan> {
    let root = parse(source);
    let linked = LinkedNode::new(&root);
    let mut result = Vec::new();
    visit(&linked, &mut result);
    result
}

fn visit(node: &LinkedNode, out: &mut Vec<HighlightSpan>) {
    if let Some(tag) = highlight(node) {
        out.push(HighlightSpan {
            byte_range: node.range(),
            color: tag_to_color(tag),
        });
    }
    // Scendi sempre nei figli — highlight() ritorna None sui nodi interni
    for child in node.children() {
        visit(&child, out);
    }
}

fn tag_to_color(tag: Tag) -> egui::Color32 {
    use Tag::*;
    match tag {
        Comment        => egui::Color32::from_rgb(88,  91,  112),
        Keyword        => egui::Color32::from_rgb(203, 166, 247),
        String         => egui::Color32::from_rgb(166, 227, 161),
        Number         => egui::Color32::from_rgb(250, 179, 135),
        Heading        => egui::Color32::from_rgb(137, 180, 250),
        Strong         => egui::Color32::from_rgb(249, 226, 175),
        Emph           => egui::Color32::from_rgb(245, 194, 231),
        Link           => egui::Color32::from_rgb(116, 199, 236),
        Raw            => egui::Color32::from_rgb(148, 226, 213),
        Label | Ref    => egui::Color32::from_rgb(180, 190, 254),
        Error          => egui::Color32::from_rgb(243, 139, 168),
        _              => egui::Color32::from_rgb(205, 214, 244),
    }
}