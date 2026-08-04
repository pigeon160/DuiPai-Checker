import { useEffect } from "react";
import Editor, { type OnMount } from "@monaco-editor/react";
import * as monaco from "monaco-editor";
import editorWorker from "monaco-editor/editor/editor.worker.js?worker";
import { loader } from "@monaco-editor/react";
import type { DslError } from "../api";

// 本地打包 Monaco（完全离线，不走 CDN）
self.MonacoEnvironment = {
  getWorker: () => new editorWorker(),
};
loader.config({ monaco });

interface Props {
  value: string;
  onChange: (v: string) => void;
  onFocusChange: (focused: boolean) => void;
  errors: DslError[];
  editorRef: React.MutableRefObject<monaco.editor.IStandaloneCodeEditor | null>;
}

export default function DslEditor({
  value,
  onChange,
  onFocusChange,
  errors,
  editorRef,
}: Props) {
  const onMount: OnMount = (editor) => {
    editorRef.current = editor;
    editor.onDidFocusEditorText(() => onFocusChange(true));
    editor.onDidBlurEditorText(() => onFocusChange(false));
  };

  useEffect(() => {
    const model = editorRef.current?.getModel();
    if (!model) return;
    const markers: monaco.editor.IMarkerData[] = errors
      .filter((e) => e.line != null)
      .map((e) => ({
        severity: monaco.MarkerSeverity.Error,
        message: e.message,
        startLineNumber: e.line as number,
        endLineNumber: e.line as number,
        startColumn: 1,
        endColumn: 1,
      }));
    monaco.editor.setModelMarkers(model, "duipai", markers);
  }, [errors, editorRef]);

  return (
    <Editor
      language="duipai-dsl"
      value={value}
      onChange={(v) => onChange(v ?? "")}
      onMount={onMount}
      theme="vs-dark"
      options={{
        fontSize: 13,
        minimap: { enabled: false },
        automaticLayout: true,
        scrollBeyondLastLine: false,
        tabSize: 2,
        wordWrap: "on",
      }}
    />
  );
}
