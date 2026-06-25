from typing import Type, List, Optional
from fastapi import FastAPI, APIRouter, Request, Depends, HTTPException
from fastapi.responses import HTMLResponse
from ..core.base import BaseModel, _MODEL_REGISTRY
from .auth import get_current_user, User
from .views import router as api_router

class AdminPanel:
    def __init__(self, title: str = "Bridge Admin"):
        self.title = title
        self.models: List[Type[BaseModel]] = []
        self.router = APIRouter(prefix="/admin")

    def register(self, model_class: Type[BaseModel]):
        self.models.append(model_class)

    def build(self) -> APIRouter:
        self.router.include_router(api_router)

        @self.router.post("/login")
        async def login(request: Request):
            form = await request.form()
            username = form.get("username")
            if username == "admin" or username == "viewer":
                return {"access_token": f"bridge_token_{username}", "token_type": "bearer"}
            raise HTTPException(status_code=400, detail="Invalid credentials")

        @self.router.get("/", response_class=HTMLResponse)
        async def admin_index(request: Request):
            model_links = "".join([f'<li><a href="/admin/{m.table}">{m.__name__}</a></li>' for m in self.models])
            return f"""
            <html>
                <head><title>{self.title}</title></head>
                <body>
                    <h1>{self.title}</h1>
                    <ul>{model_links}</ul>
                </body>
            </html>
            """

        @self.router.get("/{{table}}", response_class=HTMLResponse)
        async def admin_model_list(table: str):
            model_cls = _MODEL_REGISTRY.get(table)
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
