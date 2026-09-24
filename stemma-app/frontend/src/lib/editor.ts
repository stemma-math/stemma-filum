import { history, defaultKeymap, historyKeymap } from "@codemirror/commands";
import { bracketMatching, syntaxHighlighting } from "@codemirror/language";
import { highlightSelectionMatches, search, searchKeymap } from "@codemirror/search";
import { Annotation, Compartment, EditorState } from "@codemirror/state";
import { EditorView, highlightActiveLineGutter, keymap, lineNumbers } from "@codemirror/view";
import { typst, typstHighlighter } from "./typst";

/// The source of a line, as the person writing it sees it.
///
/// Whether it can be written in is the claim's business and is set from
/// outside; what is in it is read back whenever the line is saved,
/// composed or drawn.
export type SourceEditor = {
  readonly dom: HTMLElement;
  text(): string;
  replace(text: string): void;
  writable(yes: boolean): void;
  destroy(): void;
};

/// Marks text put in by the application, which is not an edit to save.
const replaced = Annotation.define<boolean>();

export function sourceEditor(parent: HTMLElement, text: string, changed: () => void): SourceEditor {
  const access = new Compartment();
  const allowed = (yes: boolean) => [EditorView.editable.of(yes), EditorState.readOnly.of(!yes)];
  const view = new EditorView({
    parent,
    state: EditorState.create({
      doc: text,
      extensions: [
        lineNumbers(),
        highlightActiveLineGutter(),
        history(),
        bracketMatching(),
        search({ top: true }),
        highlightSelectionMatches(),
        typst,
        syntaxHighlighting(typstHighlighter),
        EditorView.lineWrapping,
        EditorView.contentAttributes.of({ spellcheck: "false", "aria-label": "The source of this line" }),
        keymap.of([...searchKeymap, ...historyKeymap, ...defaultKeymap]),
        access.of(allowed(false)),
        EditorView.updateListener.of((update) => { if (update.docChanged && !update.transactions.some((one) => one.annotation(replaced))) changed(); }),
      ],
    }),
  });
  return {
    dom: view.dom,
    text: () => view.state.doc.toString(),
    replace: (next) => view.dispatch({ changes: { from: 0, to: view.state.doc.length, insert: next }, annotations: replaced.of(true) }),
    writable: (yes) => view.dispatch({ effects: access.reconfigure(allowed(yes)) }),
    destroy: () => view.destroy(),
  };
}
