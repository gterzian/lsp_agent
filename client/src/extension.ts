import * as path from 'path';
import { workspace, ExtensionContext, window, lm, LanguageModelChatMessage, CancellationTokenSource, chat } from 'vscode';
import {
  LanguageClient,
  LanguageClientOptions,
  ServerOptions,
  TransportKind
} from 'vscode-languageclient/node';

import * as fs from 'fs';

let client: LanguageClient;

export async function activate(context: ExtensionContext) {
  const serverPath = path.join(context.extensionPath, '../server/target/debug/server');
  
  const outputChannel = window.createOutputChannel("LSP Agent Client Log");
  outputChannel.show(true);
  outputChannel.appendLine(`[LSP Agent] Activating extension. Server path: ${serverPath}`);

  if (!fs.existsSync(serverPath)) {
    window.showErrorMessage(`LSP Server not found at: ${serverPath}`);
    outputChannel.appendLine(`[LSP Agent] Server binary not found!`);
    return;
  }
  
  // The server is implemented in node
  const serverExecutable = serverPath;
  
  // If the extension is launched in debug mode then the debug server options are used
  // Otherwise the run options are used
  const serverOptions: ServerOptions = {
    run: { command: serverExecutable, transport: TransportKind.stdio },
    debug: { command: serverExecutable, transport: TransportKind.stdio }
  };

  // Options to control the language client
  const clientOptions: LanguageClientOptions = {
    // Register the server for all documents to ensure it activates immediately for testing
    documentSelector: [{ scheme: 'file', language: '*' }],
    outputChannel: outputChannel, // <--- Use the same output channel for the server logs
    synchronize: {
      // Notify the server about file changes to '.clientrc files contained in the workspace
      fileEvents: workspace.createFileSystemWatcher('**/.clientrc')
    }
  };

  // Create the language client and start the client.
  client = new LanguageClient(
    'lspAgent',
    'LSP Agent Server',
    serverOptions,
    clientOptions
  );

  // Start the client. This will also launch the server
  outputChannel.appendLine(`[LSP Agent] Starting client...`);
  await client.start();
  outputChannel.appendLine(`[LSP Agent] Client started.`);

  client.onNotification("lsp-agent/shutdown", async () => {
      outputChannel.appendLine(`[LSP Agent] Received shutdown signal from server. Stopping client.`);
      window.showInformationMessage("LSP Agent Server has shutdown.");
      await client.stop();
  });

  // Listen for active editor changes
  window.onDidChangeActiveTextEditor(editor => {
    if (editor && editor.document) {
        const uri = editor.document.uri.toString();
        outputChannel.appendLine(`[LSP Agent] Active editor changed: ${uri}`);
        client.sendRequest("workspace/executeCommand", {
            command: "lsp-agent.active-doc",
            arguments: [uri]
        }).catch(err => {
            outputChannel.appendLine(`[LSP Agent] Failed to update active doc: ${err}`);
        });
    }
  });
  
  // Initial check
  if (window.activeTextEditor && window.activeTextEditor.document) {
      const uri = window.activeTextEditor.document.uri.toString();
      client.sendRequest("workspace/executeCommand", {
          command: "lsp-agent.active-doc",
          arguments: [uri]
      }).catch(err => {
           outputChannel.appendLine(`[LSP Agent] Failed to send initial active doc: ${err}`);
      });
  }

  client.onRequest("custom/inference", async (params: any) => {
    outputChannel.appendLine(`[LSP Agent] Received custom/inference request: ${JSON.stringify(params)}`);
    window.showInformationMessage("Agent Request: " + params.request);
    try {
        const models = await lm.selectChatModels({
            vendor: 'copilot'
        });
        
        // Find GPT-5 mini or fallback to first
        const model = models.find(m => m.name.includes('GPT-5 mini')) || models[0];
        
        if (!model) {
             return { response: "No models available" };
        }

        outputChannel.appendLine(`[LSP Agent] Using model: ${model.name} (${model.id})`);

        const messages = [LanguageModelChatMessage.User(params.request)];
        const cancelToken = new CancellationTokenSource().token;
        
        const response = await model.sendRequest(messages, {}, cancelToken);
        let fullText = "";
        
        for await (const fragment of response.text) {
            fullText += fragment;
        }
        
        outputChannel.appendLine(`[LSP Agent] Model response: ${fullText}`);

        return { 
            response: fullText
        };
    } catch (e) {
        outputChannel.appendLine(`[LSP Agent] Chat model error: ${e}`);
        return { response: "Error: " + e };
    }
  });

  const chatParticipant = chat.createChatParticipant("lsp-agent.chat", async (request, context, response, token) => {
    response.markdown("Sending request to LSP server...");
    const userPrompt = request.prompt;
    
    try {
        await client.sendRequest("workspace/executeCommand", { 
            command: "lsp-agent.log-chat", 
            arguments: [userPrompt] 
        });
        response.markdown(`\n\nRequest logged by server.`);
    } catch (err) {
        response.markdown(`\n\nFailed to send request: ${err}`);
    }
  });

  context.subscriptions.push(chatParticipant);
}

export function deactivate(): Thenable<void> | undefined {
  if (!client) {
    return undefined;
  }
  return client.stop();
}
