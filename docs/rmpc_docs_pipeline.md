# The `rmpc` Documentation Pipeline

After unpacking the `rmpc` codebase and its corresponding documentation repository, it's clear they have implemented an incredibly sophisticated and robust mechanism to keep their project documentation and source code strictly in sync.

Here is a breakdown of how the whole Astro + Starlight pipeline works.

## 1. Repository Separation
The project maintains a strict boundary between its source code and its static website:
- **Main repo (`mierak/rmpc`)**: Contains the Rust CLI, UI rendering code, standard markdown roots (`README.md`, `CHANGELOG.md`), and raw `clap` command structures.
- **Docs repo (`rmpc-org/rmpc-org.github.io`)**: A fully independent Astro + Starlight project that houses all the `.mdx` guides, configuration documentation, theme galleries, and site framework.

## 2. The Orchestration Layer (GitHub Actions)
The synchronization between the two independent repositories relies on a concept called a **Repository Dispatch**.

In the main `rmpc` repo, there is a GitHub Action `.github/workflows/docs_deploy.yml`. When code is merged to `master` that impacts generic documentation files (like `CHANGELOG.md`, `CONTRIBUTING.md`, or static UI `assets/`), this action runs a `curl` command using a GitHub Token to send an API packet to the docs repository. This packet triggers the `docs_update` event.

Over in the docs repository, their deployment script (`.github/workflows/deploy.yml`) is waiting. It listens to:
1. Manual commits to the docs repository itself.
2. The `repository_dispatch` trigger sent by the main repo.

> [!TIP]
> This creates a beautiful "fire-and-forget" continuous deployment pipeline. A change to CLI arguments in the main repo can instantly trigger a downstream website redeployment.

## 3. The Injection Mechanism (`scripts/pull`)
This is arguably the cleverest part of their setup. The `package.json` build scripts within the docs repository are overridden like this:

```json
"scripts": {
    "dev": "./scripts/pull && astro dev",
    "build": "./scripts/pull && astro check && astro build",
}
```

Whenever the website builds (locally or during GitHub Actions), it executes `./scripts/pull`. This bash script performs a `git clone` or `git pull` of the **entire main `mierak/rmpc` repository directly into the root folder of the Astro static site workspace**. 

## 4. Source-of-Truth Snippet Ripping (`utils.ts`)
Because the source repository is sitting right there in the working directory during the Astro build, the documentation site has full file-system access to the raw Rust code!

In `src/utils.ts`, they built a custom utility called `findCodeItem`:

```javascript
export function findCodeItem(code: string, type: CodeItemType, name: string, rename = name): string { 
    // Logic that scans line-by-line looking for `enum {name}` or `struct {name}` 
    // and returns the raw block from the Rust codebase.
}
```

This ensures that they don't have to manually copy and paste code blocks. When they write `.mdx` files in Starlight, they can dynamically parse and render literal Rust enumerations/structs right into the website UI components. It is physically impossible for the docs to go out of sync with the underlying codebase config keys.

## 5. Build and Publish
Once the components render their `.mdx` pages with the injected snippets from the cloned Rust repo, standard `withastro/action@v5` workflow packages the static site, and GitHub Pages deploys it on their domain.

> [!IMPORTANT]
> If we want to copy this setup for Nokkvi, the architecture requires:
> 1. Setting up a dedicated Astro + Starlight project inside our repo (or separately).
> 2. Establishing a unified way to extract our `enum` variants, `config.toml`, and Rust structures without duplicating standard text.
> 3. Linking the two with GitHub Actions so website builds auto-update.
