const vscode = require('vscode');
const { execFile } = require('child_process');
const path = require('path');

function runVetoDiagnostics(doc, collection) {
    if (!doc || doc.languageId !== 'veto' || doc.uri.scheme !== 'file') return;
    const filePath = doc.uri.fsPath;
    const workDir = path.dirname(filePath);
    const env = Object.assign({}, process.env, { VETOC_ALLOW_MANUAL: '1' });

    execFile('vetoc', [filePath, '--kernel', '--native-asm', '-o', '/tmp/veto_ide_check.s'], { cwd: workDir, env }, (err, stdout, stderr) => {
        const output = (stderr || '') + '\n' + (stdout || '');
        const diags = [];
        const regex = /\[(SYNTAX|TYPE|VETO|INQUISITOR)\s*\|\s*[^:\]]+:(\d+)\]:\s*(.+)/g;
        let match;
        while ((match = regex.exec(output)) !== null) {
            const stage = match[1];
            const lineNum = Math.max(0, parseInt(match[2], 10) - 1);
            const msg = match[3].trim();
            const lineText = lineNum < doc.lineCount ? doc.lineAt(lineNum).text : '';
            const startCol = Math.max(0, lineText.search(/\S|$/));
            const endCol = Math.max(startCol + 1, lineText.length);
            const range = new vscode.Range(lineNum, startCol, lineNum, endCol);
            const diag = new vscode.Diagnostic(range, `[${stage}] ${msg}`, vscode.DiagnosticSeverity.Error);
            diag.source = 'vetoc';
            diags.push(diag);
        }
        collection.set(doc.uri, diags);
    });
}

function activate(context) {
    // 1. Форматтер кода (Shift + Alt + F)
    const formatter = vscode.languages.registerDocumentFormattingEditProvider('veto', {
        provideDocumentFormattingEdits(document) {
            const edits = [];
            let indent = 0;

            for (let i = 0; i < document.lineCount; i++) {
                const line = document.lineAt(i);
                const trimmed = line.text.trim();

                if (trimmed.startsWith('}')) {
                    indent = Math.max(0, indent - 1);
                }

                const formatted = trimmed.length > 0 ? '    '.repeat(indent) + trimmed : '';
                if (line.text !== formatted) {
                    edits.push(vscode.TextEdit.replace(line.range, formatted));
                }

                if (trimmed.endsWith('{') && !trimmed.startsWith('//')) {
                    indent++;
                }
            }
            return edits;
        }
    });

    // 2. Умное закрытие угловых скобок для типов (i32<|>), не мешающее while x < 10
    const smartBrackets = vscode.workspace.onDidChangeTextDocument(e => {
        const editor = vscode.window.activeTextEditor;
        if (!editor || e.document !== editor.document || e.document.languageId !== 'veto') return;
        if (e.contentChanges.length !== 1 || e.contentChanges[0].text !== '<') return;
        const pos = e.contentChanges[0].range.start;
        if (pos.character === 0) return;
        const prevChar = e.document.getText(new vscode.Range(pos.line, pos.character - 1, pos.line, pos.character));
        if (/[a-zA-Z0-9_]/.test(prevChar)) {
            const insertPos = pos.translate(0, 1);
            editor.edit(eb => eb.insert(insertPos, '>'), { undoStopBefore: false, undoStopAfter: false }).then(() => {
                editor.selection = new vscode.Selection(insertPos, insertPos);
            });
        }
    });

    // 3. Автодополнение ключевых слов, типов и аппаратных примитивов по нажатию Tab
    const completionProvider = vscode.languages.registerCompletionItemProvider('veto', {
        provideCompletionItems() {
            const items = [];
            const keywords = [
                ['fn', 'fn ${1:name}(${2}) -> ${3:()} {\n    $0\n}', 'Function declaration'],
                ['let', 'let ${1:name}: ${2:i32<0..100>} = ${3:0};', 'Immutable variable'],
                ['let mut', 'let mut ${1:name}: ${2:i32<0..100>} = ${3:0};', 'Mutable variable'],
                ['while', 'while ${1:i < 10} {\n    $0\n}', 'While loop'],
                ['if', 'if ${1:cond} {\n    $0\n}', 'If branch'],
                ['struct', 'struct ${1:Name} {\n    ${2:field}: ${3:i32<0..100>},\n}', 'Struct definition'],
                ['bitstruct', 'bitstruct ${1:Name}: ${2:u8} {\n    ${3:field}: ${4:u4},\n}', 'Bitfield struct'],
                ['mmio', '#[mmio(${1:0xB8000}..${2:0xB8FA0})]', 'MMIO hardware capability'],
                ['port', '#[port(${1:0x60}..${2:0x64})]', 'I/O Port hardware capability'],
                ['use', 'use "${1:std/hal.vt}";', 'Import VETO module'],
                ['vga_print', 'vga_print(${1:row}, ${2:col}, "${3:TEXT}", ${4:0x0F});', 'Print string to VGA text buffer'],
                ['vga_poke', 'vga_poke(${1:pos}, ${2:char_code}, ${3:0x0F});', 'Write character to VGA cell'],
                ['port_in8', 'port_in8(${1:0x60})', 'Read 8-bit value from I/O port'],
                ['port_out8', 'port_out8(${1:0x64}, ${2:0xFE});', 'Write 8-bit value to I/O port']
            ];

            for (const [label, snippet, detail] of keywords) {
                const item = new vscode.CompletionItem(label, vscode.CompletionItemKind.Snippet);
                item.insertText = new vscode.SnippetString(snippet);
                item.detail = detail;
                items.push(item);
            }

            const types = ['i8', 'i16', 'i32', 'i64', 'u8', 'u16', 'u32', 'u64', 'u1', 'u2', 'u4', 'bool'];
            for (const t of types) {
                const item = new vscode.CompletionItem(t, vscode.CompletionItemKind.TypeParameter);
                item.detail = 'VETO primitive type';
                items.push(item);
            }
            return items;
        }
    });

    // 4. Живая проверка ошибок компилятора (красные волнистые линии)
    const diagCollection = vscode.languages.createDiagnosticCollection('veto');
    if (vscode.window.activeTextEditor) {
        runVetoDiagnostics(vscode.window.activeTextEditor.document, diagCollection);
    }

    context.subscriptions.push(
        formatter,
        smartBrackets,
        completionProvider,
        diagCollection,
        vscode.workspace.onDidSaveTextDocument(doc => runVetoDiagnostics(doc, diagCollection)),
        vscode.workspace.onDidOpenTextDocument(doc => runVetoDiagnostics(doc, diagCollection)),
        vscode.window.onDidChangeActiveTextEditor(ed => { if (ed) runVetoDiagnostics(ed.document, diagCollection); })
    );
}

function deactivate() {}

module.exports = { activate, deactivate };
