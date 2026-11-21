const vscode = require('vscode');

function activate(context) {
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

    context.subscriptions.push(formatter);
}

function deactivate() {}

module.exports = { activate, deactivate };