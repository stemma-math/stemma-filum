import { history, defaultKeymap, historyKeymap } from "@codemirror/commands";
import { bracketMatching, syntaxHighlighting } from "@codemirror/language";
import { Annotation, Compartment, EditorState } from "@codemirror/state";
import { EditorView, keymap, placeholder } from "@codemirror/view";
import { typst, typstHighlighter } from "./typst";

/** The CodeMirror view holding one block's Typst content. Whether it is
 * writable follows the claim and is set by the caller. */
export type BlockEditor = {
  readonly dom: HTMLElement;
  text(): string;
  replace(text: string): void;
  writable(yes: boolean): void;
  focus(): void;
  destroy(): void;
};

/** Marks text the application replaced, which is not an edit to save. */
const replaced = Annotation.define<boolean>();

/** Mounts a block's editor in `parent`; `changed` fires on user edits with
 * the new text. */
export function blockEditor(
  parent: HTMLElement,
  text: string,
  options: { label: string; empty: string; writable: boolean; changed: (text: string) => void },
): BlockEditor {
  const access = new Compartment();
  const allowed = (yes: boolean) => [EditorView.editable.of(yes), EditorState.readOnly.of(!yes)];
  const view = new EditorView({
    parent,
    state: EditorState.create({
      doc: text,
      extensions: [
        history(),
        bracketMatching(),
        typst,
        syntaxHighlighting(typstHighlighter),
        EditorView.lineWrapping,
        placeholder(options.empty),
        EditorView.contentAttributes.of({ spellcheck: "false", "aria-label": options.label }),
        keymap.of([...historyKeymap, ...defaultKeymap]),
        access.of(allowed(options.writable)),
        EditorView.updateListener.of((update) => {
          if (update.docChanged && !update.transactions.some((one) => one.annotation(replaced))) {
            options.changed(update.state.doc.toString());
          }
        }),
      ],
    }),
  });
  return {
    dom: view.dom,
    text: () => view.state.doc.toString(),
    replace: (next) => view.dispatch({ changes: { from: 0, to: view.state.doc.length, insert: next }, annotations: replaced.of(true) }),
    writable: (yes) => view.dispatch({ effects: access.reconfigure(allowed(yes)) }),
    focus: () => view.focus(),
    destroy: () => view.destroy(),
  };
}
