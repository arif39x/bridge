from datetime import datetime, timedelta
from typing import Type, List, Optional
from fastapi import FastAPI, APIRouter, Request, Depends, HTTPException
from fastapi.responses import HTMLResponse
from ..core.base import BaseModel, Registry, _MODEL_REGISTRY
from .auth import get_current_user, User, SECRET_KEY, ALGORITHM, verify_credentials
from .csrf import generate_csrf_token, CSRF_COOKIE_NAME, csrf_protect
from .ratelimit import rate_limit
from .views import router as api_router
import jwt


class AdminPanel:
    def __init__(self, title: str = "Bridge Admin", registry: Optional[Registry] = None):
        self.title = title
        self.models: List[Type[BaseModel]] = []
        self.registry = registry or _MODEL_REGISTRY
        self.router = APIRouter(prefix="/admin")

    def register(self, model_class: Type[BaseModel]):
        self.models.append(model_class)

    def build(self) -> APIRouter:
        self.router.include_router(api_router)

        @self.router.post("/login")
        async def login(request: Request):
            form = await request.form()
            username = form.get("username", "")
            password = form.get("password", "")

            user = verify_credentials(username, password)
            if user is None:
                raise HTTPException(status_code=401, detail="Invalid credentials")

            payload = {
                "sub": user.username,
                "roles": user.roles,
                "exp": datetime.utcnow() + timedelta(hours=24),
            }
            token = jwt.encode(payload, SECRET_KEY, algorithm=ALGORITHM)
            return {"access_token": token, "token_type": "bearer"}

        @self.router.get("/csrf-token")
        async def get_csrf_token():
            token = generate_csrf_token()
            return {
                CSRF_COOKIE_NAME: token,
                "token": token,
            }

        @self.router.get("/", response_class=HTMLResponse)
        async def admin_index(request: Request):
            csrf_token = generate_csrf_token()
            model_links = "".join([f'<li><a href="/admin/{m.table}">{m.__name__}</a></li>' for m in self.models])
            return f"""
            <html>
                <head><title>{self.title}</title></head>
                <body>
                    <h1>{self.title}</h1>
                    <ul>{model_links}</ul>
                    <script>
                        document.cookie = "{CSRF_COOKIE_NAME}={csrf_token}; path=/; SameSite=Strict";
                    </script>
                </body>
            </html>
            """

        @self.router.get("/{table}", response_class=HTMLResponse)
        async def admin_model_list(table: str):
            model_cls = self.registry.get(table)
            if not model_cls:
                return "Model not found", 404

            items = await model_cls.query().limit(50).fetch()
            rows = "".join([f"<tr>{''.join([f'<td>{v}</td>' for v in item.to_dict().values()])}</tr>" for item in items])
            headers = "".join([f"<th>{f}</th>" for f in model_cls._fields])

            return f"""
            <html>
                <head><title>{model_cls.__name__} - {self.title}</title></head>
                <body>
                    <h1>{model_cls.__name__}</h1>
                    <table border="1">
                        <thead><tr>{headers}</tr></thead>
                        <tbody>{rows}</tbody>
                    </table>
                    <br/><a href="/admin">Back to index</a>
                </body>
            </html>
            """

        return self.router
