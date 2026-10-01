use nana_ui::ApplicationWindow;
use nana_ui::runtime::{
    AppShell, AppTitleBar, Entity, FrameworkError, Stack,
    view::{IntoView, Refs, entity_ref, widget, with_refs},
};

pub fn mount_app_shell<V, R>(
    window: &mut ApplicationWindow,
    title: &str,
    body: impl FnOnce() -> (V, R),
) -> Result<(Entity<AppTitleBar>, R::Resolved), FrameworkError>
where
    V: IntoView,
    R: Refs,
{
    let document = window.document.document();
    let title = title.to_string();
    let (_mounted, (title_bar, body_refs)) =
        window
            .document
            .context_mut()
            .mount_view_root(document, || {
                let title_ref = entity_ref::<AppTitleBar>();
                let (body_view, body_refs) = body();
                let root = widget(AppShell::new())
                    .title_bar(widget(AppTitleBar::new(title)).entity_ref(title_ref))
                    .body(widget(Stack::fill_column(12.0).padding(16.0)).children(body_view));
                with_refs(root, (title_ref, body_refs))
            })?;
    Ok((title_bar, body_refs))
}
