# Admin Console

React/Vite administration console for the user administration platform.

## Stack

- React 18.
- TypeScript.
- Vite.
- Ant Design.
- TanStack Query.
- Zustand.
- React Router.

## Prerequisites

- Node.js compatible with the checked-in `package-lock.json`.
- Backend running at `http://127.0.0.1:8080` for local API proxying.

Install dependencies from this directory:

```bash
npm install
```

## Development

Start the local Vite server:

```bash
npm run dev
```

Default local address:

```text
http://127.0.0.1:5173/
```

Vite proxies `/api` to the local backend:

```text
/api -> http://127.0.0.1:8080
```

The admin API client sends cookies with `credentials: "include"`. For unsafe methods, it reads the `admin_csrf` cookie and sends it as `X-CSRF-Token`.

## Verification

Run these checks before handing off admin changes:

```bash
npm run lint
npm run build
```

`npm run lint` is a TypeScript no-emit check. `npm run build` runs `tsc -b` and `vite build`.

## Project Map

```text
src/api        backend API wrappers and fetch client
src/app        router setup and protected route shell
src/components shared UI helpers
src/layouts    authenticated admin layout
src/pages      route pages
src/routes     menu route and permission metadata
src/stores     auth/profile state
src/types      API response and domain types
src/utils      formatting and permission helpers
```

## Auth And Permissions

`src/api/client.ts` handles:

- Wrapped backend `ApiResponse` parsing.
- `ApiError` construction.
- Automatic refresh on `40100` and `40101`.
- CSRF header injection for unsafe methods.

`src/app/App.tsx` protects authenticated routes. It loads the profile through `/api/auth/me`, redirects unauthenticated users to `/login`, and returns a 403 page when the current route permission is not present.

Navigation visibility and direct URL access both depend on `src/routes/menu.tsx`. When adding a new protected page:

1. Add the route element in `src/app/App.tsx`.
2. Add the matching menu metadata and permission in `src/routes/menu.tsx`.
3. Add or reuse the backend API wrapper in `src/api`.
4. Keep permission names aligned with `权限点与错误码清单.md`.

## API Conventions

Admin API wrappers should call `apiRequest<T>()` and return the `data` payload type, not the full backend envelope. The fetch client owns envelope parsing and error handling.

Use absolute API paths beginning with `/api`. Avoid hard-coding hostnames in page code; Vite handles local proxying and production deployment should route `/api` to the backend.

## Build Output

Production output is written to `dist/`. The Vite config keeps vendor chunks split for Ant Design, TanStack Query, and Lucide icons, and raises the chunk warning limit to match the current admin bundle shape.
