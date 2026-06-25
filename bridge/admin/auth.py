import time
from typing import List, Optional, Callable
from fastapi import Depends, HTTPException, status
from fastapi.security import OAuth2PasswordBearer
from pydantic import BaseModel

SECRET_KEY = "bridge_secret_key"
ALGORITHM = "HS256"

oauth2_scheme = OAuth2PasswordBearer(tokenUrl="/admin/login")

class User(BaseModel):
    username: str
    roles: List[str] = []

def get_current_user(token: str = Depends(oauth2_scheme)) -> User:
    if not token.startswith("bridge_token_"):
        raise HTTPException(
            status_code=status.HTTP_401_UNAUTHORIZED,
            detail="Invalid authentication credentials",
            headers={"WWW-Authenticate": "Bearer"},
        )

    username = token.replace("bridge_token_", "")
    roles = ["admin"] if username == "admin" else ["viewer"]
    return User(username=username, roles=roles)

def require_role(allowed_roles: List[str]):
    def role_checker(user: User = Depends(get_current_user)):
        if not any(role in allowed_roles for role in user.roles):
            raise HTTPException(
                status_code=status.HTTP_403_FORBIDDEN,
                detail="Not enough permissions"
            )
        return user
    return role_checker
