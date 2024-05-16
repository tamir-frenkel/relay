import uuid
from typing import Tuple

class PublicKey:
    @staticmethod
    def parse(input: str | bytes) -> PublicKey: ...
    def verify(
        self, buf: bytes, sign: str | bytes, max_age: int | None = None
    ) -> bool: ...
    def unpack(
        self, buf: bytes, sign: str | bytes, max_age: int | None = None
    ) -> bool: ...

class SecretKey:
    @staticmethod
    def parse(input: str | bytes) -> SecretKey: ...
    def sign(self, value: bytes) -> str: ...

def generate_key_pair() -> Tuple[SecretKey, PublicKey]: ...
def generate_relay_id() -> bytes: ...
def create_register_challenge(
    data: bytes, signature: str | bytes, secret: str | bytes, max_age: int = 60
) -> dict[str, str | uuid.UUID]: ...
def validate_register_response(
    data: bytes, signature: str | bytes, secret: str | bytes, max_age: int = 60
) -> dict[str, str | uuid.UUID]: ...
def is_version_supported(version: str | bytes | None = None) -> bool:
    """
    Checks if the provided Relay version is still compatible with this library. The version can be
    ``None``, in which case a legacy Relay is assumed.
    """
    ...
