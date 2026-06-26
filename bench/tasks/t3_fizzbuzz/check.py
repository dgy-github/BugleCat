from fizzbuzz import fizzbuzz
assert fizzbuzz(5)==['1','2','Fizz','4','Buzz']
r=fizzbuzz(15)
assert r[14]=='FizzBuzz' and r[2]=='Fizz' and r[4]=='Buzz'
print('ok')
