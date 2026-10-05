from workers import WorkerEntrypoint, wsgi
from django.conf import settings
settings.configure(DEBUG=False, SECRET_KEY="framework-fixture-key", ROOT_URLCONF="app_urls", ALLOWED_HOSTS=["worker.test", ".localhost"], MIDDLEWARE=[], INSTALLED_APPS=[], DATABASES={})
import django
django.setup()
from django.core.handlers.wsgi import WSGIHandler
application = WSGIHandler()

class Default(WorkerEntrypoint):
    async def fetch(self, request):
        return await wsgi.fetch(application, request, self.env, self.ctx)
