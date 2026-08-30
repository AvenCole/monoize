# Design

## Source of truth
- Status: Active
- Last refreshed: 2026-08-30
- Primary product surfaces: Dashboard settings, authentication, provider and request operations.
- Evidence reviewed: `frontend/src/components/settings`, `frontend/src/pages/layout.tsx`, `spec/frontend-design-system.spec.md`.

## Brand
- Personality: Quiet, technical, and trustworthy.
- Trust signals: Clear state labels, restrained color, and explicit security boundaries.
- Avoid: Marketing panels, decorative gradients, and controls that imply unsupported persistence.

## Product goals
- Goals: Let administrators scan configuration state and complete common settings tasks quickly.
- Non-goals: Store SMTP secrets in the dashboard or add a second configuration authority.
- Success signals: No page overflow, clear loading/error states, and localized labels.

## Personas and jobs
- Primary personas: Administrators operating a self-hosted API gateway.
- User jobs: Configure identity, access, routing, and security settings.
- Key contexts of use: Desktop dashboard and narrow laptop/mobile widths.

## Information architecture
- Primary navigation: Sidebar sections with settings category rail.
- Core routes/screens: Login, dashboard, settings, providers, request logs.
- Content hierarchy: Category title, group label, field label, value, description, state.

## Design principles
- Principle 1: Make operational state scannable before exposing actions.
- Principle 2: Reuse native controls and tokens before adding bespoke UI.
- Tradeoffs: Dense settings panels take priority over decorative whitespace.

## Visual language
- Color: Neutral dark/light tokens with semantic success and destructive accents.
- Typography: Existing dashboard typography and localized text.
- Spacing/layout rhythm: Existing field and group spacing; compact responsive grids.
- Shape/radius/elevation: Small radii, thin borders, no nested cards.
- Motion: Existing reduced-motion-aware page transitions only.
- Imagery/iconography: Lucide icons for control affordances and state markers.

## Components
- Existing components to reuse: `Field`, `FieldDescription`, `FieldLabel`, `Input`, `Switch`, `Button`, `SettingsGroup`.
- New/changed components: Registration SMTP status panel and branded logo upload panel.
- Variants and states: Enabled, unavailable, uploading, error, configured, unconfigured.
- Token/component ownership: Tailwind utility classes and existing UI primitives.

## Accessibility
- Target standard: WCAG 2.1 AA intent.
- Keyboard/focus behavior: All controls are keyboard reachable with visible native focus.
- Contrast/readability: Semantic text colors must remain readable in both themes.
- Screen-reader semantics: Labels associate with controls; status text is explicit.
- Reduced motion and sensory considerations: Respect existing reduced-motion hooks.

## Responsive behavior
- Supported breakpoints/devices: 320px and above; desktop-first dashboard with mobile wrapping.
- Layout adaptations: Grids collapse to one column; status rows wrap without horizontal overflow.
- Touch/hover differences: Buttons retain text labels and adequate hit areas.

## Interaction states
- Loading: Disable mutation controls and show progress icon.
- Empty: Show unconfigured state with server setup guidance.
- Error: Render localized error text adjacent to the affected section.
- Success: Refresh preview/status immediately after mutation.
- Disabled: Preserve labels and explain server-managed configuration.
- Offline/slow network, if applicable: Keep existing request error handling.

## Content voice
- Tone: Direct and technical.
- Terminology: Use canonical environment variable names and SMTP.
- Microcopy rules: Explain what is stored, what is server-managed, and what action is available.

## Implementation constraints
- Framework/styling system: React, Tailwind utilities, shadcn-style primitives, react-i18next.
- Design-token constraints: Reuse existing semantic tokens and component variants.
- Performance constraints: Avoid extra polling or network requests for static configuration copy.
- Compatibility constraints: Preserve existing API contracts and email secret boundary.
- Test/screenshot expectations: Run frontend typecheck/build and inspect responsive markup.

## Open questions
- [ ] Should a future deployment manager expose server environment editing outside the dashboard? / owner: product / impact: separate security boundary.
