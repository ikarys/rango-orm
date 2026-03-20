# Django ORM Reference → Rango equivalents

Mapping complet de ce que Django ORM propose, avec notre décision pour chaque feature.
Legend: ✅ Prévu | 🔄 Différent/amélioré | ❌ Non-goal | 🤔 À décider

---

## Field options (communs à tous les champs)

| Django | Rango | Note |
|---|---|---|
| `null=True` | `Option<T>` | ✅ Rust natif, plus propre |
| `blank=True` | ❌ non-goal | Validation = domaine applicatif |
| `choices=...` | `FieldEnum<MyEnum>` | 🤔 à décider — enum Rust natif |
| `db_column="name"` | `#[field(column = "name")]` | ✅ |
| `db_comment="..."` | `#[field(comment = "...")]` | ✅ utile pour introspection DB |
| `db_default=Now()` | `#[field(db_default = "now()")]` | ✅ |
| `db_index=True` | `#[field(index)]` | ✅ |
| `default=value` | `#[field(default = "value")]` | ✅ |
| `editable=False` | `#[field(readonly)]` | 🤔 à décider |
| `primary_key=True` | `#[field(primary_key)]` | ✅ (auto si champ = `id`) |
| `unique=True` | `#[field(unique)]` | ✅ |
| `unique_for_date` | 🤔 | Rarement utilisé, à voir |
| `unique_for_month` | 🤔 | Idem |
| `unique_for_year` | 🤔 | Idem |
| `validators=[...]` | opt-in via `garde` | ❌ pas dans Rango core |
| `verbose_name` | `#[field(label = "...")]` | 🤔 utile pour API/doc |

---

## Field types

| Django | Rango | SQL généré |
|---|---|---|
| `AutoField` | auto sur `id: FieldInt` ou `id: FieldBigInt` | SERIAL / BIGSERIAL |
| `BigAutoField` | `id: FieldBigInt` (PK auto) | BIGSERIAL |
| `BooleanField` | `FieldBool` | BOOLEAN |
| `CharField(max_length=N)` | `FieldVarchar<0, N>` | VARCHAR(N) |
| `TextField` | `FieldText` | TEXT |
| `EmailField` | `FieldEmail` | VARCHAR(254) |
| `URLField` | `FieldUrl` | VARCHAR(2048) |
| `SlugField` | `FieldSlug<N>` | VARCHAR(N) 🤔 |
| `IntegerField` | `FieldInt` | INTEGER |
| `SmallIntegerField` | `FieldSmallInt` | SMALLINT |
| `BigIntegerField` | `FieldBigInt` | BIGINT |
| `PositiveIntegerField` | `FieldRange<i32, 0, 2147483647>` | INTEGER + CHECK |
| `FloatField` | `FieldFloat` | REAL |
| `DecimalField(max_digits, decimal_places)` | `FieldDecimal<P, S>` | NUMERIC(P,S) |
| `BinaryField` | `FieldBytes` | BYTEA / BLOB |
| `BooleanField` | `FieldBool` | BOOLEAN |
| `DateField` | `FieldDate` | DATE |
| `DateTimeField` | `FieldDateTime` | TIMESTAMPTZ |
| `TimeField` | `FieldTime` | TIME |
| `DurationField` | `FieldDuration` 🤔 | INTERVAL (pg) / BIGINT (autres) |
| `UUIDField` | `FieldUuid` | UUID (pg) / CHAR(36) |
| `JSONField` | `FieldJson` | JSONB (pg) / JSON |
| `FileField` | ❌ non-goal | Gérer les fichiers = hors ORM |
| `ImageField` | ❌ non-goal | Idem |
| `IPAddressField` | `FieldIpAddr` 🤔 | VARCHAR(45) |
| `GenericIPAddressField` | `FieldIpAddr` 🤔 | VARCHAR(45) |

---

## Related fields

| Django | Rango | Note |
|---|---|---|
| `ForeignKey(Model, on_delete=CASCADE)` | `ForeignKey<User>` ou `#[field(fk(User, cascade))]` | 🤔 syntaxe à définir — **vrai CASCADE SQL** |
| `OneToOneField` | `OneToOne<Model>` | ✅ |
| `ManyToManyField` | `ManyToMany<Model>` | ✅ table pivot auto-générée |
| `on_delete=CASCADE` | `on_delete = Cascade` | 🔄 **vrai SQL CASCADE**, pas du Python |
| `on_delete=SET_NULL` | `on_delete = SetNull` | ✅ |
| `on_delete=PROTECT` | `on_delete = Restrict` | ✅ |
| `related_name` | ❌ pas de magic reverse | 🔄 relations explicites uniquement |
| `through=` (M2M custom) | `through = MyPivotModel` | ✅ |

---

## Model Meta options

| Django Meta | Rango `#[model(...)]` | Note |
|---|---|---|
| `abstract = True` | `#[derive(ModelMixin)]` | 🔄 mixins explicites |
| `db_table = "name"` | `#[model(table = "name")]` | ✅ |
| `db_table_comment` | `#[model(comment = "...")]` | ✅ |
| `ordering = ["-date"]` | `#[model(ordering = ["-created_at"])]` | 🤔 |
| `unique_together = [...]` | `#[model(unique_together(field1, field2))]` | ✅ |
| `indexes = [...]` | `#[model(index(field1, field2))]` | ✅ composite index |
| `constraints = [...]` | `#[model(check(age >= 18))]` | 🤔 CHECK constraints |
| `managed = False` | `#[model(unmanaged)]` | ✅ tables externes |
| `proxy = True` | 🤔 | Proxy models — à évaluer |
| `verbose_name` | `#[model(label = "...")]` | 🤔 |

---

## RelatedManager (queryset sur relations)

| Django | Rango | Note |
|---|---|---|
| `blog.entry_set.all()` | `.related::<Entry>().all()` | 🔄 explicite, pas de magie |
| `.add(obj)` | `.add(obj)` | ✅ |
| `.create(...)` | `.create(...)` | ✅ |
| `.remove(obj)` | `.remove(obj)` | ✅ |
| `.clear()` | `.clear()` | ✅ |
| `.set([...])` | `.set([...])` | ✅ |

---

## QuerySet API (à traiter séparément)

Django propose : `filter`, `exclude`, `get`, `all`, `values`, `annotate`, `aggregate`,
`select_related`, `prefetch_related`, `order_by`, `distinct`, `count`, `exists`,
`update`, `delete`, `bulk_create`, `bulk_update`, `raw`, etc.

→ Voir `QUERYSET_REFERENCE.md` à créer.

---

## Ce que Django fait mal → ce qu'on fera mieux

- `null=True` + `blank=True` sur string fields = confusion → Rango : `Option<T>` point final
- `on_delete=CASCADE` = faux (Python loops) → Rango : vrai SQL CASCADE
- `related_name` magic = implicit, fragile → Rango : relations explicites
- `makemigrations` text-diff = parfois faux → Rango : snapshots typés
- Pas de types forts sur les champs → Rango : `FieldEmail`, `FieldVarchar<3,32>` etc.
- `Q()` objects verbeux pour AND/OR → Rango : `.and()` `.or()` natifs dans le query builder
