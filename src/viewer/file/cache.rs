use super::super::{
    breadcrumb::JsonBreadcrumbCache,
    render::{LineWindowCache, RenderedLineCache, TailPositionCache},
    syntax_state::MarkdownSyntaxCache,
};

#[derive(Default)]
pub(in crate::viewer) struct ViewerCaches {
    pub(in crate::viewer) line: LineWindowCache,
    pub(in crate::viewer) render: RenderedLineCache,
    pub(in crate::viewer) markdown: MarkdownSyntaxCache,
    pub(in crate::viewer) tail: TailPositionCache,
    pub(in crate::viewer) breadcrumb: JsonBreadcrumbCache,
}
