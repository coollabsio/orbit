# UI and interaction conventions

## Visual language

- Flat panes on a shared canvas, separated by 1px hairline borders.
- Cards are for settings sections and contained summaries, not page-sized wrappers.
- Purple is the accent in both themes.
- Use semantic tokens from `tokens.css`; avoid raw theme colors.
- Header alignment matters: first/second sidebars and pane headers use a 48px rhythm.
- Navigation rows are compact (32px) with restrained 6–10px spacing.
- Inputs inside composite controls must suppress their own ring; the outer container receives `:focus-within`.

## Sidebars

- First sidebar groups Workspace, Personal, Manage.
- Personal order is Inbox, then Direct messages.
- Collapse control is at the bottom beside the user menu.
- Collapsed state is a 56px icon rail with section separators.
- Feature sidebars may be resizable from 220–420px; persist widths in local storage and hide resizers on mobile.
- Second sidebar content starts below a 48px header with internal padding; never flush against the top border.

## Menus, modals, and stacking

- Reuse `Dropdown`, `Listbox`, and `Modal`.
- Popovers use the elevated shell, line border, 12px-ish radius, and shared shadow.
- Emoji panels must use the same `EmojiPicker`, not separately styled lookalikes.
- Emoji popovers in modals are allowed to paint outside modal scroll containers.
- Modals stay centered on both axes at every viewport size; do not switch mobile dialogs to top alignment.
- Clicking outside and Escape close menus/dialogs where appropriate.
- Destructive page/project/message operations need confirmation based on impact.

## Motion

- Keep transitions around 100–240ms.
- Animate opacity plus only a few pixels of translation or a very small scale.
- Do not animate the entire multi-pane Chat/DM page; it caused screen shake. Animate internal rows/panels instead.
- Do not apply transforms that permanently create containing blocks for fixed overlays.
- `prefers-reduced-motion` must disable practical motion.

## Chat conventions

- Message hover toolbar: top three emoji reactions plus `…` only.
- `…` opens the full context menu.
- Add Reaction opens the complete shared picker, not a small emoji strip.
- DM headers show name/avatar/presence only, not job title.
- Channel mentions visually match user mentions and navigate to the channel.
- Mobile conversations use compact 12px text, 30px avatars, restrained media widths, and an icon-first header.
- `/chat` is the mobile channel list; conversation Back navigation must return there without auto-opening a channel.

## Mobile navigation

- The bottom dock is the primary global navigation; feature-owned headers replace the global topbar on Tasks, Docs, Mail, Chat, DMs, and Inbox.
- Home keeps only Home, search, theme, and settings in its header. It intentionally has no New menu.
- Dense list headers should use compact icon controls; the Tasks title doubles as the project picker.

## Attachments

- Images can be visually large/inline and open in a lightbox.
- Non-image files should be compact chips in task descriptions, mail replies, and similar dense contexts.
- Remove and download controls must never overlap.
- Support file picker, paste, and drop whenever the surface is an editor/composer.

## Accessibility

- Keep semantic buttons/links and useful `aria-label`s.
- Ensure keyboard Escape/outside-close and focus trapping for modal flows.
- Focus rings follow the complete interactive component, not a borderless child input.
- Do not encode status only through color when accompanying labels/badges are practical.
- Respect reduced motion and mobile safe areas.
