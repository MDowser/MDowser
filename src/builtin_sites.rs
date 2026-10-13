// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `builtin_sites`: the single curated site list behind the suggestions.
//!
//! - ONE source of truth for the builtin omnibox entries:
//!   [`BUILTIN_SUGGESTIONS`], imported by `suggest` (ranking) and nowhere
//!   else. The first-run bookmark grid (`bookmarks::DEFAULT_BOOKMARKS`)
//!   keeps its own five-tile selection, but every seeded default must also
//!   appear here (enforced by `suggest`'s test), so the omnibox always
//!   offers the full set.
//! - Entries are `(url, title)` in display order: `suggest` walks them in
//!   listed order, skipping URLs already covered by bookmarks/history.
//! - URLs must be pages this browser can open (`https://`, or a local
//!   `file://` path) in their normalized store form (trailing `/` for
//!   HTTPS roots, like the stores keep them): ranking dedups by exact URL,
//!   and the test asserts `util::normalize_stored_url` keeps every entry
//!   byte-for-byte.
//!
//! TO GROW THE LIST: append one `(url, title)` line at the end of
//! [`BUILTIN_SUGGESTIONS`]. Nothing else needs to change: no count to
//! bump, no test list to mirror.

/// Curated builtin suggestions: the seeded bookmarks plus the docs kept
/// out of the first-run grid and the curated site/docs list.
/// Append new entries at the end to grow the list.
pub const BUILTIN_SUGGESTIONS: &[(&str, &str)] = &[
    ("https://acceptmarkdown.com/", "Accept Markdown"),
    (
        "https://agentready.md/leaderboard",
        "AgentReady Leaderboard",
    ),
    ("https://www.cloudflare.com/", "Cloudflare"),
    ("https://docs.stripe.com/", "Stripe Docs"),
    ("https://vercel.com/docs", "Vercel Docs"),
    ("https://isitagentready.com/", "Is It Agent Ready"),
    ("https://docs.sentry.io/", "Sentry Docs"),
    ("https://supabase.com/", "Supabase"),
    (
        "https://developers.cloudflare.com/",
        "Cloudflare Developers",
    ),
    ("https://resend.com/", "Resend"),
    ("https://airbyte.com/", "Airbyte"),
    ("https://langfuse.com/", "Langfuse"),
    ("https://lattice.com/", "Lattice"),
    ("https://vercel.com/", "Vercel"),
    ("https://vapi.ai/", "Vapi"),
    ("https://www.pagerduty.com/", "PagerDuty"),
    ("https://retool.com/", "Retool"),
    ("https://www.browserbase.com/", "Browserbase"),
    ("https://www.mintlify.com/", "Mintlify"),
    ("https://www.algolia.com/", "Algolia"),
    ("https://zapier.com/", "Zapier"),
    ("https://www.brex.com/", "Brex"),
    ("https://www.deel.com/", "Deel"),
    ("https://www.amplitude.com/", "Amplitude"),
    ("https://sentry.io/", "Sentry"),
    ("https://cal.com/", "Cal.com"),
    ("https://neon.com/", "Neon"),
    ("https://upstash.com/", "Upstash"),
    ("https://workos.com/", "WorkOS"),
    ("https://planetscale.com/", "PlanetScale"),
    ("https://www.astronomer.io/", "Astronomer"),
    ("https://clerk.com/", "Clerk"),
    ("https://exa.ai/", "Exa"),
    ("https://www.greptile.com/", "Greptile"),
    ("https://laravel.com/", "Laravel"),
    ("https://fly.io/", "Fly.io"),
    ("https://asana.com/", "Asana"),
    ("https://composio.dev/", "Composio"),
    ("https://docs.github.com/", "GitHub Docs"),
    ("https://blog.cloudflare.com/", "Cloudflare Blog"),
    ("https://docs.browser-use.com/", "Browser Use Docs"),
    ("https://docs.browserbase.com/", "Browserbase Docs"),
    ("https://docs.langchain.com/", "LangChain Docs"),
    ("https://docs.ollama.com/", "Ollama Docs"),
    ("https://docs.cohere.com/", "Cohere Docs"),
    ("https://docs.replit.com/", "Replit Docs"),
    ("https://developers.notion.com/", "Notion Developers"),
    ("https://docs.discord.com/", "Discord Docs"),
    ("https://docs.firecrawl.dev/", "Firecrawl Docs"),
    ("https://docs.baseten.co/", "Baseten Docs"),
    ("https://docs.cartesia.ai/", "Cartesia Docs"),
    (
        "https://inference-docs.cerebras.ai/",
        "Cerebras Inference Docs",
    ),
    ("https://docs.lovable.dev/", "Lovable Docs"),
    ("https://docs.poolside.ai/", "Poolside Docs"),
    ("https://docs.slack.dev/", "Slack Docs"),
    ("https://confidence.spotify.com/", "Spotify Confidence"),
    ("https://nextjs.org/", "Next.js"),
    ("https://agentgrade.com/", "AgentGrade"),
    ("https://promptwatch.com/", "Promptwatch"),
    ("https://snezzi.com/", "Snezzi"),
    ("https://is-agentic.com/", "Is-Agentic"),
    ("https://agent-ready.dev/", "Agent Ready"),
    ("https://agentready.md/", "AgentReady.md"),
    ("https://roots.io/", "Roots"),
    ("https://www.heinz.com/", "Heinz"),
    ("https://ai.virustotal.com/", "VirusTotal AI"),
    ("https://www.lusha.com/", "Lusha"),
    ("https://monogram.io/", "Monogram"),
    ("https://daliso.com/", "Daliso Ngoma"),
    ("https://israynotarray.com/", "Is Ray Not Array"),
    ("https://scholar-sidekick.com/", "Scholar Sidekick"),
    ("https://correctdocs.com/", "Correct Docs"),
    ("https://davidbuenov.com/", "David Bueno Vallejo"),
    ("https://admintoolkit.io/", "Admin Toolkit"),
    ("https://beel.es/", "BeeL"),
    ("https://zentract.io/", "Zentract"),
    (
        "https://www.poderosamentesimple.com/",
        "Poderosamente Simple",
    ),
    ("https://cyberkov.com/", "Cyberkov"),
    ("https://kamilolczyk.com/", "Kamil Olczyk"),
    ("https://webcarespro.com/", "WebCare Pro"),
    ("https://bild.systems/", "bild"),
    ("https://www.baticarla.com/", "Carla Salvador"),
    ("https://modelcontextprotocol.io/", "Model Context Protocol"),
];
