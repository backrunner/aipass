# AIPass for Microsoft Edge

## Required fields

- **Name:** AIPass
- **Short description:** Save and fill AI provider credentials through your local encrypted AIPass vault.
- **Category:** Productivity
- **Website:** https://aipass.alkinum.io/
- **Support:** https://github.com/backrunner/aipass/issues
- **Privacy policy:** https://aipass.alkinum.io/docs/security
- **Search terms (maximum 7):** password manager; API key; AI tools; developer tools; credential manager; secret manager; autofill

## English description

AIPass is a local-first browser companion for developers who use multiple AI provider consoles. Pair it with the AIPass desktop app and native messaging host to look up the right provider credential for the page you are viewing, then fill it only after you choose an entry.

The extension can match provider consoles, search your AIPass vault, and fill a selected credential into a supported page. Each fill uses a short-lived, origin-bound authorization issued by the unlocked local vault. The extension never receives your master password, and the browser extension does not persist API keys. Session cache entries contain only display-safe metadata and are cleared with the browser session.

When you create or view a supported API key in a provider console, AIPass can show a reviewable draft. Nothing is saved until you confirm it. You can edit the title, endpoint, authentication method, and tags before saving the key into the encrypted AIPass vault. You can also ignore an origin when you do not want detection or lookup on that site.

AIPass works with the local AIPass agent through Native Messaging. It does not send credentials to AIPass servers, run remote code, or include advertising. The desktop app must be installed and the vault must be unlocked for lookup, save, and fill actions. If the local host is unavailable, the extension remains installed and clearly reports that it is disconnected.

Supported workflows include provider consoles for OpenAI, Anthropic, Google Gemini, Azure OpenAI, OpenRouter, DeepSeek, Qwen, Moonshot, Zhipu, Volcengine Ark, Together, Fireworks, Groq, Replicate, New API, One API, LiteLLM, sub2api, and compatible custom endpoints. Provider matching is based on the page origin and the entries you configured in your own vault.

AIPass is open source under the Apache-2.0 license. Read the security model and setup instructions at https://aipass.alkinum.io/docs/.

## Store asset notes

The supplied 1280×800 images are prepared product-UI listing visuals for the connected and disconnected states. Re-capture them from the running popup after a visual QA pass if the layout changes before submission.
