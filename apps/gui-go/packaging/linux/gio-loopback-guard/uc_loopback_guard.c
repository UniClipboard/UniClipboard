/*
 * GIO proxy-resolver extension: loopback addresses are always direct, every other URI is answered by the
 * resolver GLib would have picked without this module (GNOME settings, libproxy: environment, PAC).
 *
 * Why this exists (docs/architecture/gui-go-linux-appimage-system-proxy.md): WebKitGTK's network process asks
 * g_proxy_resolver_get_default() for every request. The GNOME resolver only bypasses what the user's
 * `ignore-hosts` lists, so with an empty list the WebView's own connections to the local daemon (HTTP and the
 * WebSocket carrying the session token in its URL) are sent to the user's proxy. The resolver is chosen by
 * extension priority inside the network process, so only a module in GIO_MODULE_DIR can sit in front of it.
 * This module decides nothing except loopback; it is not a resolver.
 *
 * Priority 100 is above glib-networking's "gnome" (80) and "libproxy" (10), checked in 2.80.0.
 */
#include <gio/gio.h>
#include <string.h>

#define UC_TYPE_LOOPBACK_GUARD (uc_loopback_guard_get_type())
#define EXTENSION_NAME "uniclipboard-loopback"
#define EXTENSION_PRIORITY 100

typedef struct {
  GObject parent_instance;
  GMutex lock;
  GProxyResolver *loopback; /* GSimpleProxyResolver: answers direct:// for ignored (loopback) hosts only */
  GProxyResolver *inner;    /* the resolver GLib would have picked without this module */
  gboolean inner_searched;
} UcLoopbackGuard;

typedef struct {
  GObjectClass parent_class;
} UcLoopbackGuardClass;

static GType uc_loopback_guard_get_type(void);
static void uc_loopback_guard_proxy_resolver_iface_init(GProxyResolverInterface *iface);

G_DEFINE_DYNAMIC_TYPE_EXTENDED(UcLoopbackGuard, uc_loopback_guard, G_TYPE_OBJECT, 0,
                               G_IMPLEMENT_INTERFACE_DYNAMIC(G_TYPE_PROXY_RESOLVER, uc_loopback_guard_proxy_resolver_iface_init))

static void uc_loopback_guard_init(UcLoopbackGuard *self) {
  static const char *const loopback[] = {"localhost", "127.0.0.0/8", "::1", NULL};
  g_mutex_init(&self->lock);
  /* GLib's own matcher (hostnames, CIDR, IPv6 literals) rather than a private one: a sentinel default proxy
   * makes "ignored" observable as direct://. */
  self->loopback = g_simple_proxy_resolver_new("http://loopback-sentinel.invalid:1", (char **)loopback);
}

static void uc_loopback_guard_finalize(GObject *object) {
  UcLoopbackGuard *self = (UcLoopbackGuard *)object;
  g_clear_object(&self->loopback);
  g_clear_object(&self->inner);
  g_mutex_clear(&self->lock);
  G_OBJECT_CLASS(uc_loopback_guard_parent_class)->finalize(object);
}

static void uc_loopback_guard_class_init(UcLoopbackGuardClass *klass) {
  G_OBJECT_CLASS(klass)->finalize = uc_loopback_guard_finalize;
}

static void uc_loopback_guard_class_finalize(UcLoopbackGuardClass *klass) { (void)klass; }

/* The next supported implementation of the extension point below this one, instantiated once. g_proxy_resolver_get_default()
 * must not be used here: it returns this very object. */
static GProxyResolver *inner_resolver(UcLoopbackGuard *self) {
  GProxyResolver *found = NULL;
  g_mutex_lock(&self->lock);
  if (!self->inner_searched) {
    GIOExtensionPoint *point = g_io_extension_point_lookup(G_PROXY_RESOLVER_EXTENSION_POINT_NAME);
    for (GList *l = point ? g_io_extension_point_get_extensions(point) : NULL; l && !self->inner; l = l->next) {
      GIOExtension *ext = l->data;
      if (g_io_extension_get_priority(ext) >= EXTENSION_PRIORITY)
        continue;
      GObject *object = g_object_new(g_io_extension_get_type(ext), NULL);
      if (G_IS_INITABLE(object) && !g_initable_init(G_INITABLE(object), NULL, NULL)) {
        g_object_unref(object);
        continue;
      }
      if (g_proxy_resolver_is_supported(G_PROXY_RESOLVER(object)))
        self->inner = G_PROXY_RESOLVER(object);
      else
        g_object_unref(object);
    }
    self->inner_searched = TRUE;
  }
  found = self->inner;
  g_mutex_unlock(&self->lock);
  return found;
}

static gboolean is_loopback(UcLoopbackGuard *self, const gchar *uri) {
  GError *error = NULL;
  gchar **answer = g_proxy_resolver_lookup(self->loopback, uri, NULL, &error);
  gboolean direct = answer && answer[0] && strcmp(answer[0], "direct://") == 0;
  g_strfreev(answer);
  g_clear_error(&error);
  return direct;
}

static gboolean uc_is_supported(GProxyResolver *resolver) {
  (void)resolver;
  return TRUE;
}

static gchar **uc_lookup(GProxyResolver *resolver, const gchar *uri, GCancellable *cancellable, GError **error) {
  UcLoopbackGuard *self = (UcLoopbackGuard *)resolver;
  GProxyResolver *inner;
  if (is_loopback(self, uri))
    return g_strdupv((gchar *[]){"direct://", NULL});
  inner = inner_resolver(self);
  if (!inner)
    return g_strdupv((gchar *[]){"direct://", NULL});
  return g_proxy_resolver_lookup(inner, uri, cancellable, error);
}

static void uc_lookup_async(GProxyResolver *resolver, const gchar *uri, GCancellable *cancellable, GAsyncReadyCallback callback,
                            gpointer user_data) {
  UcLoopbackGuard *self = (UcLoopbackGuard *)resolver;
  GProxyResolver *inner = is_loopback(self, uri) ? NULL : inner_resolver(self);
  if (inner) {
    /* The callback receives the inner resolver's own GAsyncResult: uc_lookup_finish hands it back. */
    g_proxy_resolver_lookup_async(inner, uri, cancellable, callback, user_data);
    return;
  }
  GTask *task = g_task_new(resolver, cancellable, callback, user_data);
  g_task_return_pointer(task, g_strdupv((gchar *[]){"direct://", NULL}), (GDestroyNotify)g_strfreev);
  g_object_unref(task);
}

static gchar **uc_lookup_finish(GProxyResolver *resolver, GAsyncResult *result, GError **error) {
  UcLoopbackGuard *self = (UcLoopbackGuard *)resolver;
  if (g_task_is_valid(result, resolver))
    return g_task_propagate_pointer(G_TASK(result), error);
  return g_proxy_resolver_lookup_finish(inner_resolver(self), result, error);
}

static void uc_loopback_guard_proxy_resolver_iface_init(GProxyResolverInterface *iface) {
  iface->is_supported = uc_is_supported;
  iface->lookup = uc_lookup;
  iface->lookup_async = uc_lookup_async;
  iface->lookup_finish = uc_lookup_finish;
}

G_MODULE_EXPORT void g_io_module_load(GIOModule *module) {
  g_type_module_use(G_TYPE_MODULE(module));
  uc_loopback_guard_register_type(G_TYPE_MODULE(module));
  g_io_extension_point_implement(G_PROXY_RESOLVER_EXTENSION_POINT_NAME, UC_TYPE_LOOPBACK_GUARD, EXTENSION_NAME, EXTENSION_PRIORITY);
}

G_MODULE_EXPORT void g_io_module_unload(GIOModule *module) { (void)module; }

G_MODULE_EXPORT gchar **g_io_module_query(void) {
  gchar *eps[] = {(gchar *)G_PROXY_RESOLVER_EXTENSION_POINT_NAME, NULL};
  return g_strdupv(eps);
}
