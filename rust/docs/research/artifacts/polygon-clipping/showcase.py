import sys
def get(path, cid):
    res=[]; on=False
    for line in open(path):
        if line.startswith("CASE "):
            on = (int(line.split()[1])==cid)
            if not on and res: break
            continue
        if on:
            if line.strip()=="END": break
            res.append(line.rstrip("\n"))
    return res
cid=int(sys.argv[1])
a=get(sys.argv[2],cid); b=get(sys.argv[3],cid)
import difflib
for l in difflib.unified_diff(a,b,lineterm="",n=int(sys.argv[4]) if len(sys.argv)>4 else 1):
    print(l[:400])
