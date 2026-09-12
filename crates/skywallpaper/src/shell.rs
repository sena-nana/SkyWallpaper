use nana_ui::ApplicationWindow;
use nana_ui::runtime::{AppShell, AppTitleBar, Entity, FrameworkError, Stack, UiBuilder};

pub fn mount_app_shell<R>(
    window: &mut ApplicationWindow,
    title: &str,
    body: impl FnOnce(&mut UiBuilder<'_>, Entity<AppTitleBar>) -> R,
) -> Result<R, FrameworkError> {
    let document = window.document.document();
    let (result, shell) = window.document.context_mut().build(document, |ui| {
        let shell = ui.child("shell", AppShell::new());
        let result = ui.nest(shell, |ui| {
            let title_bar = ui.child("title", AppTitleBar::new(title));
            ui.with("body", Stack::fill_column(12.0).padding(16.0), |ui| {
                body(ui, title_bar)
            })
        });
        (result, shell)
    })?;
    window.document.context_mut().assemble_app_shell(shell)?;
    Ok(result)
}
