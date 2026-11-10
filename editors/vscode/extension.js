const fs = require("node:fs");
const path = require("node:path");
const vscode = require("vscode");

function compilerFor(document, folder) {
    const configured = vscode.workspace
        .getConfiguration("mammoth", document.uri)
        .get("compilerPath", "")
        .trim();
    if (configured) {
        return /[\\/]/.test(configured)
            ? path.resolve(folder?.uri.fsPath ?? path.dirname(document.uri.fsPath), configured)
            : configured;
    }
    const executable = process.platform === "win32" ? "mammoth.exe" : "mammoth";
    if (folder) {
        const local = path.join(folder.uri.fsPath, "target", "release", executable);
        if (fs.existsSync(local)) {
            return local;
        }
    }
    // The Windows setup's default folder, which a VS Code window opened before
    // the install cannot find on its older PATH.
    if (process.platform === "win32" && process.env.LOCALAPPDATA) {
        const installed = path.join(process.env.LOCALAPPDATA, "Programs", "Mammoth", "bin", executable);
        if (fs.existsSync(installed)) {
            return installed;
        }
    }
    return executable;
}

async function run(operation) {
    const document = vscode.window.activeTextEditor?.document;
    if (!vscode.workspace.isTrusted) {
        await vscode.window.showErrorMessage("Trust this workspace before running Mammoth.");
        return;
    }
    if (!document || document.languageId !== "mammoth" || document.uri.scheme !== "file") {
        await vscode.window.showInformationMessage("Open a saved .mam file first.");
        return;
    }
    if (document.isDirty && !(await document.save())) {
        await vscode.window.showErrorMessage("Save the Mammoth file before running it.");
        return;
    }

    const folder = vscode.workspace.getWorkspaceFolder(document.uri);
    const cwd = folder?.uri.fsPath ?? path.dirname(document.uri.fsPath);
    const args = operation === "check-java" ? ["--check-java", document.uri.fsPath] : [document.uri.fsPath];
    const execution = new vscode.ProcessExecution(compilerFor(document, folder), args, { cwd });
    const task = new vscode.Task(
        { type: "mammoth", operation, file: document.uri.fsPath },
        folder ?? vscode.TaskScope.Global,
        operation === "check-java" ? "Check Java Bindings" : "Run File",
        "Mammoth",
        execution,
        ["$mammoth"],
    );
    task.presentationOptions = { reveal: vscode.TaskRevealKind.Always, panel: vscode.TaskPanelKind.Dedicated, clear: true };
    await vscode.tasks.executeTask(task);
}

function activate(context) {
    context.subscriptions.push(
        vscode.commands.registerCommand("mammoth.runFile", () => run("run")),
        vscode.commands.registerCommand("mammoth.checkJava", () => run("check-java")),
    );
}

module.exports = { activate };
