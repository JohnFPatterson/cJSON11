/*
 * Public-API oracle driver for the cJSON C-to-Rust port.
 * Links against the unmodified cJSON.c / cJSON.h.
 * Report format: tools/DRIVER_FORMAT.md
 */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <stdbool.h>

#include "../cJSON.h"

#define COMPARE_COST_LIMIT 1000000ULL

typedef struct {
    bool parse;
    bool print;
    bool tree;
} sections_t;

static void usage(const char *argv0)
{
    fprintf(stderr, "Usage: %s [--sections parse,print,tree] <file>\n", argv0);
}

static int parse_sections(const char *arg, sections_t *out)
{
    const char *p = arg;
    out->parse = out->print = out->tree = false;
    while (*p != '\0')
    {
        const char *start = p;
        size_t len;
        while (*p != '\0' && *p != ',')
        {
            p++;
        }
        len = (size_t)(p - start);
        if (len == 5 && strncmp(start, "parse", 5) == 0)
        {
            out->parse = true;
        }
        else if (len == 5 && strncmp(start, "print", 5) == 0)
        {
            out->print = true;
        }
        else if (len == 4 && strncmp(start, "tree", 4) == 0)
        {
            out->tree = true;
        }
        else
        {
            return 0;
        }
        if (*p == ',')
        {
            p++;
        }
    }
    return out->parse || out->print || out->tree;
}

static char *read_file(const char *path, long *out_len)
{
    FILE *f = fopen(path, "rb");
    long len;
    char *buf;
    size_t nread;
    if (f == NULL)
    {
        return NULL;
    }
    if (fseek(f, 0, SEEK_END) != 0)
    {
        fclose(f);
        return NULL;
    }
    len = ftell(f);
    if (len < 0)
    {
        fclose(f);
        return NULL;
    }
    if (fseek(f, 0, SEEK_SET) != 0)
    {
        fclose(f);
        return NULL;
    }
    buf = (char *)malloc((size_t)len + 1);
    if (buf == NULL)
    {
        fclose(f);
        return NULL;
    }
    nread = fread(buf, 1, (size_t)len, f);
    fclose(f);
    if ((long)nread != len)
    {
        free(buf);
        return NULL;
    }
    buf[len] = '\0';
    if (out_len != NULL)
    {
        *out_len = len;
    }
    return buf;
}

/* Cost estimate for cJSON_Compare: identical decision on both drivers. */
static unsigned long long estimate_compare_cost(const cJSON *item)
{
    unsigned long long cost = 1;
    const cJSON *child;
    if (item == NULL)
    {
        return 1;
    }
    if ((item->type & (cJSON_Array | cJSON_Object)) != 0)
    {
        for (child = item->child; child != NULL; child = child->next)
        {
            unsigned long long child_cost = estimate_compare_cost(child);
            /* Object compare can be quadratic/exponential in siblings; bound conservatively. */
            if ((item->type & cJSON_Object) != 0)
            {
                if (child_cost > COMPARE_COST_LIMIT)
                {
                    return COMPARE_COST_LIMIT + 1;
                }
                cost += child_cost * 2;
            }
            else
            {
                cost += child_cost;
            }
            if (cost > COMPARE_COST_LIMIT)
            {
                return COMPARE_COST_LIMIT + 1;
            }
        }
    }
    return cost;
}

int main(int argc, char **argv)
{
    sections_t sections = { true, true, true };
    const char *path = NULL;
    char *input = NULL;
    long input_len = 0;
    cJSON *root = NULL;
    int i;

    for (i = 1; i < argc; i++)
    {
        if (strcmp(argv[i], "--sections") == 0)
        {
            if (i + 1 >= argc)
            {
                usage(argv[0]);
                return 2;
            }
            i++;
            if (!parse_sections(argv[i], &sections))
            {
                usage(argv[0]);
                return 2;
            }
        }
        else if (path == NULL)
        {
            path = argv[i];
        }
        else
        {
            usage(argv[0]);
            return 2;
        }
    }
    if (path == NULL)
    {
        usage(argv[0]);
        return 2;
    }

    input = read_file(path, &input_len);
    if (input == NULL)
    {
        fprintf(stderr, "failed to read %s\n", path);
        return 2;
    }

    root = cJSON_ParseWithLength(input, (size_t)input_len);

    if (sections.parse)
    {
        printf("=== parse ===\n");
        if (root != NULL)
        {
            printf("status: ok\n");
            printf("error_offset: -\n");
        }
        else
        {
            const char *ep = cJSON_GetErrorPtr();
            long off = 0;
            if (ep != NULL && ep >= input && ep <= input + input_len)
            {
                off = (long)(ep - input);
            }
            printf("status: fail\n");
            printf("error_offset: %ld\n", off);
        }
    }

    if (sections.print)
    {
        char *compact = NULL;
        char *pretty = NULL;
        printf("=== print ===\n");
        printf("compact:\n");
        if (root != NULL)
        {
            compact = cJSON_PrintUnformatted(root);
            if (compact != NULL)
            {
                fputs(compact, stdout);
            }
        }
        printf("\nend_compact\n");
        printf("pretty:\n");
        if (root != NULL)
        {
            pretty = cJSON_Print(root);
            if (pretty != NULL)
            {
                fputs(pretty, stdout);
            }
        }
        printf("\nend_pretty\n");
        if (compact != NULL)
        {
            cJSON_free(compact);
        }
        if (pretty != NULL)
        {
            cJSON_free(pretty);
        }
    }

    if (sections.tree)
    {
        char *minbuf;
        printf("=== tree ===\n");
        if (root != NULL)
        {
            printf("array_size: %d\n", cJSON_GetArraySize(root));
            if (estimate_compare_cost(root) > COMPARE_COST_LIMIT)
            {
                printf("compare_self: skipped\n");
            }
            else if (cJSON_Compare(root, root, 1))
            {
                printf("compare_self: same\n");
            }
            else
            {
                printf("compare_self: diff\n");
            }
        }
        else
        {
            printf("array_size: -\n");
            printf("compare_self: skipped\n");
        }
        printf("minify:\n");
        minbuf = (char *)malloc((size_t)input_len + 1);
        if (minbuf != NULL)
        {
            memcpy(minbuf, input, (size_t)input_len + 1);
            cJSON_Minify(minbuf);
            fputs(minbuf, stdout);
            free(minbuf);
        }
        printf("\nend_minify\n");
    }

    if (root != NULL)
    {
        cJSON_Delete(root);
    }
    free(input);
    return 0;
}
