import * as monaco from "monaco-editor";

export const DSL_LANGUAGE = "duipai-dsl";

const COMMANDS = [
  "line", "int", "float", "text", "expr", "str",
  "ints", "floats", "matrix", "matf", "perm", "tree", "graph",
  "binseq", "intervals", "points", "ring", "base_ring",
];

/** 命令补全模板（insertText 用 tabstop 语法）。 */
const SNIPPETS: Record<string, { body: string; doc: string }> = {
  repeat: {
    body: "repeat (${1:N}):\n    ${2:line}:\n        ${3:int} ${4:n}: ${5:1}, ${6:100}",
    doc: "repeat 块：整体重复 N 次，变量每轮覆盖（优先级最高）",
  },
  line: { body: "line:\n    ${1:int} ${2:n}: ${3:1}, ${4:100}", doc: "行块：一行多个数（可重复）" },
  int: { body: "int ${1:n}: ${2:1}, ${3:100}", doc: "行内整数项" },
  float: { body: "float ${1:x}: ${2:0}, ${3:1}, ${4:prec}", doc: "行内浮点项" },
  text: { body: "text ${1:s}: \"${2:---}\"", doc: "行内固定文本" },
  expr: { body: "expr ${1:e}: ${2:2 * n}", doc: "行内自由表达式" },
  str: { body: "str ${1:s}: ${2:10}, \"${3:ab}\"", doc: "行内字符串（长度可随机）" },
  ints: { body: "ints(${1:count}, ${2:min}, ${3:max})", doc: "数组：一行 count 个整数" },
  floats: { body: "floats(${1:count}, ${2:lo}, ${3:hi}, ${4:prec})", doc: "数组：一行 count 个浮点" },
  matrix: { body: "matrix(${1:rows}, ${2:cols}, ${3:min}, ${4:max})", doc: "矩阵：rows × cols 整数" },
  matf: { body: "matf(${1:rows}, ${2:cols}, ${3:lo}, ${4:hi}, ${5:prec})", doc: "矩阵：rows × cols 浮点" },
  perm: { body: "perm(${1:n})", doc: "排列 1..n" },
  tree: { body: "tree(${1:n}, int(${2:1}, ${3:100}))", doc: "树（type=\"star\"/\"chain\" 可指定结构）" },
  graph: {
    body: "graph(${1:n}, ${2:m}, ${3:directed}, ${4:connected}, int(${5:1}, ${6:100}))",
    doc: "图，multi=1/loop=1/type= 可选项",
  },
  binseq: { body: "binseq(${1:n}, ${2:k})", doc: "0/1 序列：n 位中 k 个 1" },
  intervals: { body: "intervals(${1:n}, ${2:lo}, ${3:hi})", doc: "区间，n 行 l r" },
  points: { body: "points(${1:n}, ${2:xlo}, ${3:xhi}, ${4:ylo}, ${5:yhi})", doc: "点集，n 行 x y" },
  ring: { body: "ring(${1:n})", doc: "环：n 顶点首尾相连" },
  base_ring: { body: "base_ring(${1:n}, ${2:k})", doc: "基环树：n 顶点，环大小 k" },
};

let registered = false;

/** 注册 DSL 语言（语法高亮 + 补全）。变量补全通过 ref 动态提供。 */
export function registerDslLanguage(
  varNamesRef: () => string[],
  onApply: () => void,
) {
  if (registered) return;
  registered = true;

  monaco.languages.register({ id: DSL_LANGUAGE });

  monaco.languages.setMonarchTokensProvider(DSL_LANGUAGE, {
    keywords: COMMANDS,
    tokenizer: {
      root: [
        [/#.*$/, "comment"],
        [/"(?:[^"\\]|\\.)*"/, "string"],
        [/\d+\.?\d*|\.\d+/, "number"],
        [/[a-zA-Z_][a-zA-Z0-9_]*/, { cases: { "@keywords": "keyword", "@default": "identifier" } }],
        [/[+\-*/%=(),:]/, "delimiter"],
      ],
    },
  });

  monaco.languages.setLanguageConfiguration(DSL_LANGUAGE, {
    comments: { lineComment: "#" },
    brackets: [["(", ")"]],
  });

  monaco.languages.registerCompletionItemProvider(DSL_LANGUAGE, {
    triggerCharacters: [" ", "("],
    provideCompletionItems: (model, position) => {
      const word = model.getWordUntilPosition(position);
      const range = new monaco.Range(
        position.lineNumber,
        word.startColumn,
        position.lineNumber,
        word.endColumn,
      );
      const suggestions: monaco.languages.CompletionItem[] = COMMANDS.map((cmd) => {
        const s = SNIPPETS[cmd];
        return {
          label: cmd,
          kind: monaco.languages.CompletionItemKind.Function,
          detail: s.doc,
          insertText: s.body,
          insertTextRules: monaco.languages.CompletionItemInsertTextRule.InsertAsSnippet,
          range,
        };
      });
      for (const name of varNamesRef()) {
        suggestions.push({
          label: name,
          kind: monaco.languages.CompletionItemKind.Variable,
          detail: "引用前面定义的变量（取规模值）",
          insertText: name,
          range,
        });
      }
      return { suggestions };
    },
  });

  // Ctrl+Enter 应用 DSL（与“应用”按钮同行为）
  monaco.editor.addEditorAction({
    id: "duipai.apply",
    label: "应用 DSL",
    keybindings: [monaco.KeyMod.CtrlCmd | monaco.KeyCode.Enter],
    run: () => onApply(),
  });
}
