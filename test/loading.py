import time, sys
def loading():
    print("Loading...")
    for i in range(0, 100):
        time.sleep(0.5)
        sys.stdout.write(u"\u001b[1000D")
        sys.stdout.write("0" * 70 + str(i + 1) + "%")
        sys.stdout.write(u"\u001b[1A")
        sys.stdout.write(u"\u001b[1B")
        sys.stdout.flush()
    print()
    
loading()
