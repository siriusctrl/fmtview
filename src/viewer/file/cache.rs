use super::{
    breadcrumb::JsonBreadcrumbCache,
    markdown_modes::MarkdownModeCache,
    render::{LineWindowCache, RenderedLineCache, TailPositionCache},
};

#[derive(Default)]
pub(in crate::viewer) struct ViewerCaches {
    pub(in crate::viewer) line: LineWindowCache,
    pub(in crate::viewer) render: RenderedLineCache,
    pub(in crate::viewer) markdown: MarkdownModeCache,
    pub(in crate::viewer) tail: TailPositionCache,
    pub(in crate::viewer) breadcrumb: JsonBreadcrumbCache,
}
